//! Moc: a region of the sky made of HEALPix cells, as the IVOA MOC
//! (Multi-Order Coverage) recommendation describes one (moc.c).
//!
//! A Moc holds a sorted list of disjoint ranges of HEALPix nested indices at
//! one order, its MaxOrder, in ICRS. Cells added at a lower order are stored
//! as the range of their descendants at MaxOrder, so a Moc's resolution is
//! fixed by MaxOrder. The normalised form, the list of NUNIQ values the MOC
//! recommendation describes, is derived from the ranges when it is needed.
//!
//! Like any Region, a Moc may be given another celestial coordinate system
//! through its FrameSet; its cells stay in ICRS, its base Frame. A Moc has no
//! uncertainty of its own; its default uncertainty is a Circle whose radius
//! is the resolution at MaxOrder.
//!
//! The per-class methods moc.c gives a Moc, and how they map here:
//!
//! - Architecture, each a trait method: `Transform` (through
//!   `reg_base_inside`), `Equal`, `RegBaseBox`, `RegBaseMesh`, `RegPins` and
//!   `GetDefUnc`. A Moc inherits region.c's `Simplify` and `GetBounded`.
//! - Ceremony: the attribute string dispatch (`MaxOrder`, `MaxRes`,
//!   `MinOrder`, `MinRes`, `MocArea`, `MocLength`, `MocType` are typed
//!   accessors), the copy constructor and destructor, and the dump and load
//!   functions.
//!
//! Not ported yet: `RegTrace`, which only `Plot` uses, and with it the trace
//! of each perimeter that moc.c `RegBaseMesh` makes after the mesh points are
//! found (moc.c:7672-8170, the `dist` and `meshdist` values). The mesh points
//! themselves do not depend on it.
//!
//! The MOC forms are `add_moc_string`, `get_moc_string`, `add_moc_data`,
//! `get_moc_data` and `get_moc_header`, with the text reader and writer
//! (moc.c `astAddMocText` and `astGetMocText`) that MocChan shares.
//!
//! `add_region` and `add_pixel_mask` are moc.c `AddRegion` and
//! `AddPixelMask<X>`, with `PutCell`, `IncorporateCells`, `FindSkyAxes`,
//! `GetPixelArea` and `TestPixels`. Like C they map the Region with
//! `reg_map_region`, which does not simplify, since inside the library
//! `astMapRegion` is the unsimplifying `astMapRegion_`.

use std::cell::RefCell;

use crate::constants::{BAD_VALUE, PACKAGE_STRING};
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::fitschan::FitsChan;
use crate::frames::{
    AxisKind, DynFrame, Frame, FrameSet, SkyFrame, SkySystem, simplify_frame_via_mapping,
};
use crate::mappings::helpers::is_bad;
use crate::mappings::{
    CmpMap, Composition, DynMapping, Mapping, MatrixMap, ShiftMap, UnitMap, ViaSimplify, WcsMap,
    WcsProjection,
};

use super::data::{RegionData, RegionGeometry};
use super::{CircleBuilder, CmpOperator, DynRegion, PixelOper, PixelValue, Region};
use crate::frames::FrameRef;
use crate::mappings::{XphMap, XphProjection};

/// The highest HEALPix order a Moc may use (xphmap.h `AST__MXORDHPX`).
pub const MOC_MAX_ORDER: i32 = 27;

/// The Dec at the corners where three HEALPix cells meet (moc.c `THETAX`).
const THETAX: f64 = 0.729727656226966;

/// How far inside a cell its corners are taken to be, as a fraction of a
/// cell, to keep them off discontinuities (moc.c `HALF`).
const HALF: f64 = 0.49999;

const DPI: f64 = std::f64::consts::PI;
const DPIBY2: f64 = std::f64::consts::FRAC_PI_2;
const DD2R: f64 = DPI / 180.0;

/// The largest nested index at `order` (moc.c `MaxNpix`).
fn max_npix(order: i32) -> i64 {
    12 * (1_i64 << (2 * order)) - 1
}

/// The mean size of a cell at `order`, in arc-seconds (moc.c `OrderToRes`).
fn order_to_res(order: i32) -> f64 {
    if !(0..=MOC_MAX_ORDER).contains(&order) {
        return 0.0;
    }
    211076.29 / f64::from(1_i32 << order)
}

/// The order whose cells have about the size `res`, in arc-seconds (moc.c
/// `ResToOrder`). The divisor is C's literal for ln 2, to seven figures.
#[allow(clippy::approx_constant)]
fn res_to_order(res: f64) -> i32 {
    if res <= 0.0 {
        return 0;
    }
    (0.5 + (211076.29 / res).ln() / 0.6931472) as i32
}

/// The 1-based grid indices, in an HPX12 projection at `order`, of the cell
/// with nested index `nested`, or `(-1, -1)` for an index past the twelfth
/// facet (moc.c `NestedToXy`).
fn nested_to_xy(nested: i64, order: i32) -> (i32, i32) {
    let fi = nested >> (2 * order);
    if fi >= 12 {
        return (-1, -1);
    }
    let tj = nested & ((1_i64 << (2 * order)) - 1);
    let mut tx = (tj >> 1) & 0x5555555555555555;
    let mut ty = tj & 0x5555555555555555;
    tx = (tx | (tx >> 1)) & 0x3333333333333333;
    tx = (tx | (tx >> 2)) & 0x0F0F0F0F0F0F0F0F;
    tx = (tx | (tx >> 4)) & 0x00FF00FF00FF00FF;
    tx = (tx | (tx >> 8)) & 0x0000FFFF0000FFFF;
    tx = (tx | (tx >> 16)) & 0x00000000FFFFFFFF;
    ty = (ty | (ty >> 1)) & 0x3333333333333333;
    ty = (ty | (ty >> 2)) & 0x0F0F0F0F0F0F0F0F;
    ty = (ty | (ty >> 4)) & 0x00FF00FF00FF00FF;
    ty = (ty | (ty >> 8)) & 0x0000FFFF0000FFFF;
    ty = (ty | (ty >> 16)) & 0x00000000FFFFFFFF;
    let nppf = 1_i64 << order;
    let tx = nppf - 1 - tx;
    let fi = fi as i32;
    let (fx, fy) = if fi < 4 {
        (fi, fi + 1)
    } else if fi < 8 {
        (fi - 4, fi - 4)
    } else {
        (fi - 7, fi - 8)
    };
    (
        (tx + i64::from(fx) * nppf + 1) as i32,
        (ty + i64::from(fy) * nppf + 1) as i32,
    )
}

/// The nested index of the cell at 1-based grid indices `(ix, iy)` in an
/// HPX12 projection at `order`, or `i64::MAX` for a position off the
/// projection (moc.c `XyToNested`). The arithmetic wraps as C's `int` does
/// for the saturated value a bad position rounds to.
fn xy_to_nested(order: i32, ix: i32, iy: i32) -> i64 {
    let nppf = 1_i32 << order;
    let ix = ix.wrapping_sub(1);
    let iy = iy.wrapping_sub(1);
    let mut fx = ix / nppf;
    let mut fy = iy / nppf;
    let mut tx = i64::from(ix) - i64::from(nppf) * i64::from(fx);
    let mut ty = i64::from(iy) - i64::from(nppf) * i64::from(fy);
    if fx == 4 && fy == 4 {
        fx = 0;
        fy = 0;
    }
    if !((0..5).contains(&fx) && (0..5).contains(&fy) && fx >= fy - 1 && fx <= fy + 1) {
        return i64::MAX;
    }
    let fi = if fx != 4 || fy != 4 {
        (fx + fy) / 2 + (fx - fy + 1) * 4
    } else {
        4
    };
    tx = i64::from(nppf) - 1 - tx;
    tx = (tx | (tx << 16)) & 0x0000FFFF0000FFFF;
    tx = (tx | (tx << 8)) & 0x00FF00FF00FF00FF;
    tx = (tx | (tx << 4)) & 0x0F0F0F0F0F0F0F0F;
    tx = (tx | (tx << 2)) & 0x3333333333333333;
    tx = (tx | (tx << 1)) & 0x5555555555555555;
    ty = (ty | (ty << 16)) & 0x0000FFFF0000FFFF;
    ty = (ty | (ty << 8)) & 0x00FF00FF00FF00FF;
    ty = (ty | (ty << 4)) & 0x0F0F0F0F0F0F0F0F;
    ty = (ty | (ty << 2)) & 0x3333333333333333;
    ty = (ty | (ty << 1)) & 0x5555555555555555;
    (i64::from(fi) << (2 * order)) | ty | (tx << 1)
}

/// The Mapping from ICRS (RA, Dec) to 1-based grid coordinates in an HPX12
/// projection of the whole sky at `order`, in which each pixel is one
/// HEALPix cell (moc.c `GetHPX12FrameSet`, inverted as `GetCachedMapping`
/// takes it from the FrameSet).
fn hpx12_mapping(order: i32) -> AstResult<DynMapping> {
    let mut map = hpx12_grid_to_icrs(order)?;
    map.invert();
    Ok(Box::new(map))
}

/// The Mapping from HPX12 grid coordinates at `order` to ICRS, as moc.c
/// `GetHPX12FrameSet` builds it.
fn hpx12_grid_to_icrs(order: i32) -> AstResult<CmpMap> {
    let nppf = f64::from(1_i32 << order);
    let s = -0.5 * (1.0 + 5.0 * nppf);
    let sm1 = ShiftMap::new(vec![s, s]);
    let m = DPI / (4.0 * nppf);
    let mm = MatrixMap::new(ndarray::array![[m, m], [-m, m]])?;
    let mut wm = WcsMap::new(2, WcsProjection::Hpx, 1, 2)?;
    wm.set_pv(1, 3, 180.0)?;
    wm.set_lon_check(false);
    wm.invert();
    let sm2 = ShiftMap::new(vec![DPI, 0.0]);
    let map1 = CmpMap::new(Box::new(sm1), Box::new(mm), Composition::Series)?;
    let map2 = CmpMap::new(Box::new(map1), Box::new(wm), Composition::Series)?;
    CmpMap::new(Box::new(map2), Box::new(sm2), Composition::Series)
}

/// A value derived from a Moc's ranges and MaxOrder, kept until they change
/// (moc.c `ClearCache`).
#[derive(Debug, Default)]
struct MocCache {
    /// The normalised form (moc.c `inorm` or `knorm`) and whether it uses 4
    /// byte values.
    norm: Option<(Vec<i64>, bool)>,
    /// The boundary mesh in ICRS.
    basemesh: Option<Vec<Vec<f64>>>,
    /// The bounding box in ICRS.
    bounds: Option<([f64; 2], [f64; 2])>,
    /// The area, in square arc-minutes.
    area: Option<f64>,
    /// The default uncertainty.
    unc: Option<DynRegion>,
    /// The ICRS to HPX12 grid Mapping at each order, which depends only on
    /// the order and so survives a cleared cache.
    maps: Vec<Option<DynMapping>>,
}

impl Clone for MocCache {
    /// moc.c `Copy`: the derived values are copied and the Mappings are not.
    fn clone(&self) -> Self {
        MocCache {
            norm: self.norm.clone(),
            basemesh: self.basemesh.clone(),
            bounds: self.bounds,
            area: self.area,
            unc: self.unc.as_ref().map(|u| u.clone_region()),
            maps: Vec::new(),
        }
    }
}

/// A region of the sky made of HEALPix cells (C's `Moc`).
#[derive(Debug, Clone)]
pub struct Moc {
    pub(crate) data: RegionData,
    /// The HEALPix order of the ranges (C's `maxorder`), when set.
    pub(crate) max_order: Option<i32>,
    /// The order at which to search when adding a Region (C's `minorder`),
    /// when set.
    pub(crate) min_order: Option<i32>,
    /// The ranges of nested indices at MaxOrder, `(low, high)` pairs in
    /// increasing order (C's `range`).
    pub(crate) range: Vec<i64>,
    cache: RefCell<MocCache>,
}

impl Default for Moc {
    fn default() -> Self {
        Self::new()
    }
}

impl Moc {
    /// An empty Moc, describing no area of the sky (C's `astMoc`).
    pub fn new() -> Self {
        let frame: DynFrame = Box::new(SkyFrame::new(SkySystem::Icrs));
        Moc {
            data: RegionData::new(FrameSet::new(frame), Vec::new()),
            max_order: None,
            min_order: None,
            range: Vec::new(),
            cache: RefCell::new(MocCache::default()),
        }
    }

    /// A Moc from the parts a dump gives.
    pub(crate) fn from_parts(
        data: RegionData,
        max_order: Option<i32>,
        min_order: Option<i32>,
        range: Vec<i64>,
    ) -> Self {
        Moc {
            data,
            max_order,
            min_order,
            range,
            cache: RefCell::new(MocCache::default()),
        }
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

    /// The number of ranges.
    fn nrange(&self) -> usize {
        self.range.len() / 2
    }

    /// moc.c `ClearCache`.
    fn clear_cache(&self) {
        let mut cache = self.cache.borrow_mut();
        cache.norm = None;
        cache.basemesh = None;
        cache.bounds = None;
        cache.area = None;
        cache.unc = None;
        self.data.invalidate_cache();
    }

    // ------------------------------------------------------------------
    // Attributes
    // ------------------------------------------------------------------

    /// The HEALPix order of the Moc's finest cells, -1 until it is set
    /// (`MaxOrder`).
    pub fn max_order(&self) -> i32 {
        self.max_order.unwrap_or(-1)
    }

    /// Whether `MaxOrder` has been set.
    pub fn test_max_order(&self) -> bool {
        self.max_order.is_some()
    }

    /// Set `MaxOrder`, as `astSet` does: AST__NOWRT once it is set, and
    /// AST__INVAR for an order outside 0 to [`MOC_MAX_ORDER`].
    pub fn set_max_order(&mut self, value: i32) -> AstResult<()> {
        if self.test_max_order() {
            return Err(self.max_order_fixed(&format!("MaxOrder={value}"), "MaxOrder"));
        }
        self.store_max_order(value)
    }

    /// Clear `MaxOrder`, as `astClear` does: AST__NOWRT once it is set.
    pub fn clear_max_order(&mut self) -> AstResult<()> {
        if self.test_max_order() {
            return Err(AstError::new(
                ErrorCode::Nowrt,
                "astClear: Invalid attempt to clear the \"maxorder\" value for a Moc.",
            )
            .with_context("The previously set value cannot be changed."));
        }
        self.max_order = None;
        self.clear_cache();
        Ok(())
    }

    /// The best resolution of the Moc, in arc-seconds, 0 until `MaxOrder`
    /// is set (`MaxRes`).
    pub fn max_res(&self) -> f64 {
        if self.test_max_order() {
            order_to_res(self.max_order())
        } else {
            0.0
        }
    }

    /// Set `MaxRes`, the order whose cells are nearest `res` arc-seconds in
    /// size becoming `MaxOrder`: AST__NOWRT once `MaxOrder` is set.
    pub fn set_max_res(&mut self, res: f64) -> AstResult<()> {
        if self.test_max_order() {
            return Err(self.max_order_fixed(&format!("MaxRes={res}"), "MaxRes"));
        }
        self.store_max_order(res_to_order(res))
    }

    /// The error `astSet` reports for a MaxOrder or MaxRes setting once
    /// MaxOrder is set.
    fn max_order_fixed(&self, setting: &str, name: &str) -> AstError {
        AstError::new(
            ErrorCode::Nowrt,
            format!("astSet: The setting \"{setting}\" is invalid for a Moc."),
        )
        .with_context(format!(
            "The previously set \"{name}\" value cannot be changed."
        ))
    }

    /// moc.c `SetMaxOrder`, which `astSet` and the methods that add cells
    /// use.
    pub(crate) fn store_max_order(&mut self, value: i32) -> AstResult<()> {
        if !(0..=MOC_MAX_ORDER).contains(&value) {
            return Err(AstError::new(
                ErrorCode::Invar,
                format!(
                    "astSetMaxOrder(Moc): Invalid value ({value}) supplied for parameter \
                     'MaxOrder'."
                ),
            ));
        }
        self.max_order = Some(value);
        self.clear_cache();
        Ok(())
    }

    /// The order at which to search when adding a Region, by default five
    /// less than `MaxOrder` and at least 0 (`MinOrder`).
    pub fn min_order(&self) -> i32 {
        self.min_order
            .unwrap_or_else(|| (self.max_order() - 5).max(0))
    }

    /// Whether `MinOrder` has been set.
    pub fn test_min_order(&self) -> bool {
        self.min_order.is_some()
    }

    /// Set `MinOrder`, limited to 0 to [`MOC_MAX_ORDER`].
    pub fn set_min_order(&mut self, value: i32) {
        self.min_order = Some(value.clamp(0, MOC_MAX_ORDER));
    }

    /// Clear `MinOrder`.
    pub fn clear_min_order(&mut self) {
        self.min_order = None;
    }

    /// The resolution, in arc-seconds, at which to search when adding a
    /// Region (`MinRes`).
    pub fn min_res(&self) -> f64 {
        order_to_res(self.min_order())
    }

    /// Set `MinRes`, the order whose cells are nearest `res` arc-seconds in
    /// size becoming `MinOrder`.
    pub fn set_min_res(&mut self, res: f64) {
        self.set_min_order(res_to_order(res));
    }

    /// The area of the Moc, in square arc-minutes (`MocArea`).
    pub fn moc_area(&self) -> f64 {
        if let Some(area) = self.cache.borrow().area {
            return area;
        }
        let count: i64 = self.range.chunks(2).map(|r| r[1] - r[0] + 1).sum();
        let res = order_to_res(self.max_order()) / 60.0;
        let area = res * res * count as f64;
        self.cache.borrow_mut().area = Some(area);
        area
    }

    /// The number of cells in the normalised Moc, the table length used to
    /// describe it in FITS (`MocLength`).
    pub fn moc_length(&self) -> AstResult<usize> {
        self.with_norm("astGetMocLength", |norm, _| norm.len())
    }

    /// The number of bytes in each value of the normalised Moc, 4 or 8, the
    /// data type used to describe it in FITS (`MocType`). A Moc with no
    /// cells has no 4 byte form, so it is 8.
    pub fn moc_type(&self) -> AstResult<i32> {
        self.with_norm("astGetMocType", |norm, is32| {
            if is32 && !norm.is_empty() { 4 } else { 8 }
        })
    }

    /// Run `f` on the normalised form, making it first if needed (moc.c
    /// `GetNorm`): the NUNIQ value of every cell of the MOC, the cells at
    /// each order being those whole cells the ranges hold that a lower order
    /// did not take. The values are 4 byte ones below order 14.
    fn with_norm<T>(&self, method: &str, f: impl FnOnce(&[i64], bool) -> T) -> AstResult<T> {
        if self.cache.borrow().norm.is_none() {
            let maxorder = self.max_order();
            let mut ranges: Vec<i64> = self.range.clone();
            let mut norm: Vec<i64> = Vec::new();
            let mut order = 0;
            while order <= maxorder && !ranges.is_empty() {
                let nuniq_offset = 1_i64 << (2 * (order + 1));
                let shift = 2 * (maxorder - order);
                let offset = (1_i64 << shift) - 1;
                let mut newranges = Vec::new();
                for r in ranges.chunks(2) {
                    let (min, max) = (r[0], r[1]);
                    let mut m1 = (min + offset) >> shift;
                    let mut m2 = (max + 1) >> shift;
                    if m2 > m1 {
                        if (i32::MAX as i64) - (norm.len() as i64) < m2 - m1 + 2 {
                            return Err(AstError::new(
                                ErrorCode::Bgmoc,
                                format!("{method}(Moc): The normalised MOC has too many elements."),
                            ));
                        }
                        norm.extend((m1..m2).map(|npix| nuniq_offset + npix));
                        m1 <<= shift;
                        m2 = (m2 << shift) - 1;
                        if m1 <= min {
                            if m2 < max {
                                newranges.extend([m2 + 1, max]);
                            }
                        } else if m2 >= max {
                            newranges.extend([min, m1 - 1]);
                        } else {
                            newranges.extend([min, m1 - 1, m2 + 1, max]);
                        }
                    } else {
                        newranges.extend([min, max]);
                    }
                }
                ranges = newranges;
                order += 1;
            }
            self.cache.borrow_mut().norm = Some((norm, maxorder < 14));
        }
        let cache = self.cache.borrow();
        let (norm, is32) = cache.norm.as_ref().expect("normalised form made above");
        Ok(f(norm, *is32))
    }

    // ------------------------------------------------------------------
    // Cells
    // ------------------------------------------------------------------

    /// Combine one cell with the Moc (C's `astAddCell`): AND removes it, OR
    /// adds it, and XOR toggles it. Its order must be no greater than
    /// `MaxOrder`, which it becomes if `MaxOrder` is not set; AST__INVAR for
    /// an order or `npix` out of range.
    pub fn add_cell(&mut self, cmode: CmpOperator, order: i32, npix: i64) -> AstResult<()> {
        if !(0..=MOC_MAX_ORDER).contains(&order) {
            return Err(AstError::new(
                ErrorCode::Invar,
                format!(
                    "astAddCell(Moc): Invalid value ({order}) supplied for parameter 'order' - \
                     must be no greater than {MOC_MAX_ORDER}."
                ),
            ));
        }
        if npix < 0 || npix > max_npix(order) {
            return Err(AstError::new(
                ErrorCode::Invar,
                format!(
                    "astAddCell(Moc): Invalid value ({npix}) supplied for parameter 'npix' - \
                     must be greater than 0 and less than {}.",
                    max_npix(order) + 1
                ),
            ));
        }
        let maxorder = if self.test_max_order() {
            let maxorder = self.max_order();
            if maxorder < order {
                return Err(AstError::new(
                    ErrorCode::Invar,
                    format!(
                        "astAddCell(Moc): Invalid value ({order}) supplied for parameter \
                         'order' - must be no greater than the Moc's MaxOrder attribute \
                         ({maxorder})."
                    ),
                ));
            }
            maxorder
        } else {
            self.store_max_order(order)?;
            order
        };
        let shift = 2 * (maxorder - order);
        let irange = self.nrange();
        self.range
            .extend([npix << shift, ((npix + 1) << shift) - 1]);
        self.moc_norm(
            cmode == CmpOperator::And,
            cmode,
            irange,
            maxorder,
            "astAddCell",
        )
    }

    /// moc.c `astMocNorm`: sort and merge ranges `nold` onwards, replace them
    /// with the gaps between them if `negate`, and combine them with the
    /// earlier ranges by `cmode`.
    pub(crate) fn moc_norm(
        &mut self,
        negate: bool,
        cmode: CmpOperator,
        nold: usize,
        maxorder: i32,
        method: &str,
    ) -> AstResult<()> {
        self.merge_ranges(nold);
        if negate {
            self.negate_ranges(nold, maxorder);
        }
        self.combine_ranges(cmode, method)
    }

    /// moc.c `MergeRanges`: sort ranges `start` onwards by their lower bound
    /// and merge those that touch or overlap.
    fn merge_ranges(&mut self, start: usize) {
        let nrange = self.nrange();
        if nrange > 1 && start + 1 < nrange {
            let mut pairs: Vec<[i64; 2]> = self.range[2 * start..]
                .chunks(2)
                .map(|r| [r[0], r[1]])
                .collect();
            pairs.sort_by_key(|p| p[0]);
            let mut merged: Vec<[i64; 2]> = vec![pairs[0]];
            for p in &pairs[1..] {
                let last = merged.last_mut().expect("merged starts non-empty");
                if p[0] <= last[1] + 1 {
                    if p[1] > last[1] {
                        last[1] = p[1];
                    }
                } else {
                    merged.push(*p);
                }
            }
            self.range.truncate(2 * start);
            self.range.extend(merged.iter().flatten());
        }
        self.clear_cache();
    }

    /// moc.c `NegateRanges`: replace ranges `start` onwards with the gaps
    /// between them, at `order`.
    fn negate_ranges(&mut self, start: usize, order: i32) {
        let nrange = self.nrange();
        if nrange == 0 || start >= nrange {
            return;
        }
        let max_nest = 12 * (1_i64 << (2 * order));
        let r = &mut self.range;
        if r[2 * start] == 0 {
            let mut irange = start;
            while irange < nrange {
                let p = 2 * irange;
                if r[p + 1] >= max_nest - 1 {
                    break;
                } else if irange < nrange - 1 {
                    r[p] = r[p + 1] + 1;
                    r[p + 1] = r[p + 2] - 1;
                } else {
                    r[p] = r[p + 1] + 1;
                    r[p + 1] = max_nest - 1;
                }
                irange += 1;
            }
            r.truncate(2 * irange);
        } else {
            let mut istart = 0;
            for irange in start..nrange {
                let p = 2 * irange;
                let next_start = r[p + 1] + 1;
                r[p + 1] = r[p] - 1;
                r[p] = istart;
                istart = next_start;
            }
            if istart < max_nest {
                r.extend([istart, max_nest - 1]);
            }
        }
    }

    /// moc.c `CombineRanges`: combine all the ranges, the end points of each
    /// sorted together, by union (OR), intersection (AND) or exclusive
    /// disjunction (XOR), and merge adjacent results.
    fn combine_ranges(&mut self, cmode: CmpOperator, method: &str) -> AstResult<()> {
        let _ = method;
        let inter = |message: String| AstError::new(ErrorCode::Inter, message);
        if self.nrange() > 1 {
            // (value, is the start of a range), starts sorting before ends
            // at the same value.
            let mut list: Vec<(i64, bool)> = self
                .range
                .iter()
                .enumerate()
                .map(|(i, &v)| (v, i % 2 == 0))
                .collect();
            list.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));

            let mut new: Vec<i64> = Vec::new();
            let mut depth = 0_i32;
            match cmode {
                CmpOperator::Or => {
                    for &(value, start) in &list {
                        if start {
                            if depth == 0 {
                                new.push(value);
                            }
                            depth += 1;
                        } else {
                            depth -= 1;
                            if depth == 0 {
                                new.push(value);
                            } else if depth < 0 {
                                return Err(inter(format!(
                                    "CombineRanges(Moc): Negative depth at end-point {value} \
                                     (internal programming error)."
                                )));
                            }
                        }
                    }
                }
                CmpOperator::And => {
                    for &(value, start) in &list {
                        depth += if start { 1 } else { -1 };
                        if depth > 2 {
                            return Err(inter(format!(
                                "CombineRanges(Moc): More than 2 active ranges at {value} \
                                 (internal programming error)."
                            )));
                        } else if depth == 2 {
                            new.extend([value, 0]);
                        } else if depth == 1 && !start {
                            let n = new.len();
                            new[n - 1] = value;
                        } else if depth < 0 {
                            return Err(inter(format!(
                                "CombineRanges(Moc): Negative depth at {value} (internal \
                                 programming error)."
                            )));
                        }
                    }
                }
                CmpOperator::Xor => {
                    // C keeps a pointer to the current range, which a
                    // dropped range leaves in place for the next to overwrite.
                    let mut nnew = 0_usize;
                    let mut cur = 0_usize;
                    for &(value, start) in &list {
                        depth += if start { 1 } else { -1 };
                        if depth > 2 {
                            return Err(inter(format!(
                                "CombineRanges(Moc): More than 2 active ranges at {value} \
                                 (internal programming error)."
                            )));
                        } else if depth == 2 {
                            new[cur + 1] = value - 1;
                            if new[cur + 1] < new[cur] {
                                nnew -= 1;
                            }
                        } else if depth == 1 {
                            new.resize(2 * (nnew + 1), 0);
                            cur = 2 * nnew;
                            new[cur] = value + i64::from(!start);
                            nnew += 1;
                        } else if depth < 0 {
                            return Err(inter(format!(
                                "CombineRanges(Moc): Negative depth at {value} (internal \
                                 programming error)."
                            )));
                        } else {
                            new[cur + 1] = value;
                            if new[cur + 1] < new[cur] {
                                nnew -= 1;
                            }
                        }
                    }
                    new.truncate(2 * nnew);
                }
            }
            if depth != 0 {
                return Err(inter(format!(
                    "CombineRanges(Moc): Un-balanced ranges - final depth {depth} (internal \
                     programming error)."
                )));
            }
            self.range = new;

            // Check the ranges, and merge adjacent ones.
            if !self.range.is_empty() {
                let r = &self.range;
                if r[1] < r[0] {
                    return Err(inter(format!(
                        "CombineRanges(Moc): Range 0 [{}:{}] - upper bound lower than lower \
                         bound (internal programming error).",
                        r[0], r[1]
                    )));
                }
                let mut merged: Vec<i64> = vec![r[0], r[1]];
                for irange in 1..self.nrange() {
                    let (lo, hi) = (r[2 * irange], r[2 * irange + 1]);
                    let (plo, phi) = (r[2 * irange - 2], r[2 * irange - 1]);
                    if hi < lo {
                        return Err(inter(format!(
                            "CombineRanges(Moc): Range {irange} [{lo}:{hi}] - upper bound \
                             lower than lower bound (internal programming error)."
                        )));
                    } else if phi >= lo {
                        return Err(inter(format!(
                            "CombineRanges(Moc): Range {} [{plo}:{phi}] overlaps range \
                             {irange} [{lo}:{hi}] (internal programming error).",
                            irange - 1
                        )));
                    } else if phi == lo - 1 {
                        let n = merged.len();
                        merged[n - 1] = hi;
                    } else {
                        merged.extend([lo, hi]);
                    }
                }
                self.range = merged;
            }
        }
        self.clear_cache();
        Ok(())
    }

    /// Whether a cell is in the Moc (C's `astTestCell`). A cell at an order
    /// greater than `MaxOrder` is not. Unless `parent` is set, a cell whose
    /// parent is wholly in the Moc is not either. AST__INVAR for an order or
    /// `npix` out of range.
    pub fn test_cell(&self, order: i32, npix: i64, parent: bool) -> AstResult<bool> {
        if !(0..=MOC_MAX_ORDER).contains(&order) {
            return Err(AstError::new(
                ErrorCode::Invar,
                format!(
                    "astTestCell(Moc): Invalid value ({order}) supplied for parameter 'order' \
                     - must be no greater than {MOC_MAX_ORDER}."
                ),
            ));
        }
        if npix < 0 || npix > max_npix(order) {
            return Err(AstError::new(
                ErrorCode::Invar,
                format!(
                    "astTestCell(Moc): Invalid value ({npix}) supplied for parameter 'npix' - \
                     must be greater than 0 and less than {}.",
                    max_npix(order) + 1
                ),
            ));
        }
        let maxorder = self.max_order();
        if order > maxorder {
            return Ok(false);
        }
        let covers = |shift: i32, npix: i64| {
            let ilow = npix << shift;
            let ihigh = ((npix + 1) << shift) - 1;
            self.range.chunks(2).any(|r| r[0] <= ilow && r[1] >= ihigh)
        };
        let shift = 2 * (maxorder - order);
        let mut result = covers(shift, npix);
        if result && !parent && covers(shift + 2, npix >> 2) {
            result = false;
        }
        Ok(result)
    }

    /// The order and `npix` of cell `icell`, counting from zero, of the
    /// normalised Moc (C's `astGetCell`); AST__INVAR for an index out of
    /// range.
    pub fn get_cell(&self, icell: i32) -> AstResult<(i32, i64)> {
        let moclen = self.moc_length()? as i32;
        if icell < 0 {
            return Err(AstError::new(
                ErrorCode::Invar,
                format!(
                    "astGetCell(Moc): Invalid value ({icell} zero-based) supplied for parameter \
                     'icell' - must be greater than or equal to zero."
                ),
            ));
        }
        if icell >= moclen {
            return Err(AstError::new(
                ErrorCode::Invar,
                format!(
                    "astGetCell(Moc): Invalid value ({icell} zero-based) supplied for parameter \
                     'icell' - must be less than {moclen}."
                ),
            ));
        }
        self.with_norm("astGetCell", |norm, _| decode_nuniq(norm[icell as usize]))
    }

    /// The ICRS to HPX12 grid Mapping at `order` (moc.c `GetCachedMapping`).
    fn cached_mapping(&self, order: i32) -> AstResult<DynMapping> {
        let o = order as usize;
        {
            let cache = self.cache.borrow();
            if let Some(Some(map)) = cache.maps.get(o) {
                return Ok(map.clone_box());
            }
        }
        let map = hpx12_mapping(order)?;
        let mut cache = self.cache.borrow_mut();
        if cache.maps.len() <= o {
            cache.maps.resize_with(o + 1, || None);
        }
        cache.maps[o] = Some(map.clone_box());
        Ok(map)
    }
}

/// The order and `npix` a NUNIQ value encodes (the decoding moc.c
/// `GetCell` and `RegBaseMesh` make with a fast log2).
fn decode_nuniq(nuniq: i64) -> (i32, i64) {
    let order = (63 - (nuniq / 4).leading_zeros() as i32) / 2;
    (order, nuniq - (1_i64 << (2 + 2 * order)))
}

// ============================================================================
// The boundary mesh
// ============================================================================

/// A HEALPix cell in the mesh calculation (moc.c `Cell`).
#[derive(Debug, Clone, Copy, Default)]
struct MeshCell {
    corners: [usize; 4],
    ix: i32,
    iy: i32,
    interior: bool,
    isolated: bool,
}

/// A corner position shared by up to four cells (moc.c `Corner`).
#[derive(Debug, Clone, Default)]
struct MeshCorner {
    ra: f64,
    dec: f64,
    cosdec: f64,
    cells: Vec<usize>,
    interior: bool,
    isolated: bool,
}

/// moc.c `Comp_corner`: Dec first, within `tol`, then RA, within `tol`
/// scaled by the larger cos(Dec) unless `exact`; RA taken in [-PI,PI[ for
/// the second of the two sorts.
fn comp_corner(a: &MeshCorner, b: &MeshCorner, tol: f64, exact: bool, loop_: bool) -> i32 {
    let (mut ra1, mut ra2) = (a.ra, b.ra);
    if loop_ {
        while ra1 < -DPI {
            ra1 += 2.0 * DPI;
        }
        while ra2 < -DPI {
            ra2 += 2.0 * DPI;
        }
        while ra1 >= DPI {
            ra1 -= 2.0 * DPI;
        }
        while ra2 >= DPI {
            ra2 -= 2.0 * DPI;
        }
    }
    let ratol = if exact {
        0.0
    } else if a.cosdec > b.cosdec {
        tol / a.cosdec
    } else {
        tol / b.cosdec
    };
    if (a.dec - b.dec).abs() <= tol {
        if (ra1 - ra2).abs() <= ratol {
            // In an exact sort, corners with the same RA are ordered by Dec,
            // so that the order does not depend on how a sort treats ties.
            if exact && a.dec < b.dec {
                -1
            } else if exact && a.dec > b.dec {
                1
            } else {
                0
            }
        } else if ra1 < ra2 {
            -1
        } else {
            1
        }
    } else if a.dec < b.dec {
        -1
    } else {
        1
    }
}

/// A stable merge sort by a comparison that need not be a total order, as
/// the corner comparison, with its tolerance on Dec, is not.
fn merge_sort_by(v: &mut [usize], cmp: &impl Fn(usize, usize) -> i32) {
    let n = v.len();
    if n < 2 {
        return;
    }
    let mid = n / 2;
    merge_sort_by(&mut v[..mid], cmp);
    merge_sort_by(&mut v[mid..], cmp);
    let mut merged = Vec::with_capacity(n);
    let (mut i, mut j) = (0, mid);
    while i < mid && j < n {
        if cmp(v[j], v[i]) < 0 {
            merged.push(v[j]);
            j += 1;
        } else {
            merged.push(v[i]);
            i += 1;
        }
    }
    merged.extend_from_slice(&v[i..mid]);
    merged.extend_from_slice(&v[j..n]);
    v.copy_from_slice(&merged);
}

/// The cells and corners of one boundary-mesh calculation.
struct Mesh<'a> {
    moc: &'a Moc,
    tol: f64,
    cells: Vec<MeshCell>,
    corners: Vec<MeshCorner>,
}

impl Mesh<'_> {
    /// moc.c `MakeCell`: a new cell, appended to `chain`.
    fn make_cell(&mut self, ix: i32, iy: i32, chain: &mut Vec<usize>) -> usize {
        self.cells.push(MeshCell {
            ix,
            iy,
            ..Default::default()
        });
        let id = self.cells.len() - 1;
        chain.push(id);
        id
    }

    /// moc.c `MakeCorners`: the corners of the cells in `chain`, coincident
    /// ones merged, flagged interior where four cells (or three, at the
    /// eight places three meet) share them, and, if `isolated`, flagged
    /// isolated where they touch only one cell or two diagonally. Returns
    /// the corners in C's chain order, sorted by Dec then RA if `sort`.
    fn make_corners(
        &mut self,
        order: i32,
        chain: &[usize],
        sort: bool,
        isolated: bool,
    ) -> AstResult<Vec<usize>> {
        // C walks a cell chain from its last cell back.
        let cell_order: Vec<usize> = chain.iter().rev().copied().collect();
        let ncell = cell_order.len();
        if ncell > (i32::MAX / 4 - 2) as usize {
            return Err(AstError::new(
                ErrorCode::Bgmoc,
                "astRegBaseMesh(Moc): The normalised MOC has too many elements.",
            ));
        }
        let nppf = 1_i32 << order;
        let (s1, s2, lim) = (nppf + 1, 9 * nppf + 1, nppf);
        let mut grid = PointSet::zeros(4 * ncell, 2);
        let mut cross = false;
        for (icell, &c) in cell_order.iter().enumerate() {
            let (ix, iy) = (f64::from(self.cells[c].ix), f64::from(self.cells[c].iy));
            for (k, (dx, dy)) in [(-HALF, -HALF), (-HALF, HALF), (HALF, HALF), (HALF, -HALF)]
                .into_iter()
                .enumerate()
            {
                let p = grid.point_mut(4 * icell + k);
                p[0] = ix + dx;
                p[1] = iy + dy;
            }
            let sxy = self.cells[c].ix + self.cells[c].iy;
            if (sxy - s1).abs() <= lim || (sxy - s2).abs() <= lim {
                cross = true;
            }
        }
        let map = self.moc.cached_mapping(order)?;
        let sky = map.transform(&grid, false)?;

        let base = self.corners.len();
        let mut list: Vec<usize> = Vec::with_capacity(4 * ncell);
        for (icell, &c) in cell_order.iter().enumerate() {
            for k in 0..4 {
                let p = sky.point(4 * icell + k);
                let (mut ra, dec) = (p[0], p[1]);
                if is_bad(ra) || is_bad(dec) {
                    return Err(AstError::new(
                        ErrorCode::Inter,
                        "astRegBaseMesh(Moc): Bad sky coords at corner (internal programming \
                         error).",
                    ));
                }
                while ra >= 2.0 * DPI {
                    ra -= 2.0 * DPI;
                }
                while ra < 0.0 {
                    ra += 2.0 * DPI;
                }
                self.corners.push(MeshCorner {
                    ra,
                    dec,
                    cosdec: dec.cos(),
                    cells: vec![c],
                    ..Default::default()
                });
                let id = self.corners.len() - 1;
                self.cells[c].corners[k] = id;
                list.push(id);
            }
        }
        let _ = base;

        let tol = self.tol;
        for loop_ in 0..=usize::from(cross) {
            let loop_ = loop_ == 1;
            let mut index = list.clone();
            {
                let corners = &self.corners;
                merge_sort_by(&mut index, &|a, b| {
                    comp_corner(&corners[a], &corners[b], tol, true, loop_)
                });
            }
            let mut removed = vec![false; index.len()];
            let mut root = index[0];
            for (i, &cur) in index.iter().enumerate().skip(1) {
                if comp_corner(&self.corners[root], &self.corners[cur], tol, false, loop_) == 0 {
                    if self.corners[root].cells.len() + self.corners[cur].cells.len() > 4 {
                        return Err(AstError::new(
                            ErrorCode::Inter,
                            "astRegBaseMesh(Moc): Corner is used more than 4 times (internal \
                             programming error).",
                        ));
                    }
                    let moved = std::mem::take(&mut self.corners[cur].cells);
                    for &c in &moved {
                        let Some(k) = self.cells[c].corners.iter().position(|&x| x == cur) else {
                            return Err(AstError::new(
                                ErrorCode::Inter,
                                "astRegBaseMesh(Moc): Orphaned Corner (internal programming \
                                 error).",
                            ));
                        };
                        self.cells[c].corners[k] = root;
                    }
                    self.corners[root].cells.extend(moved);
                    removed[i] = true;
                } else {
                    root = cur;
                }
            }
            let gone: std::collections::HashSet<usize> = index
                .iter()
                .zip(&removed)
                .filter(|&(_, &r)| r)
                .map(|(&c, _)| c)
                .collect();
            if sort {
                list = index;
            }
            list.retain(|c| !gone.contains(c));
        }

        for &id in &list {
            let interior_cell = self.corners[id]
                .cells
                .iter()
                .any(|&c| self.cells[c].interior);
            let corner = &mut self.corners[id];
            if interior_cell {
                corner.interior = true;
            }
            if !corner.interior {
                if corner.cells.len() == 3 {
                    let dectol = tol;
                    let ratol = dectol / THETAX.cos();
                    let ra = corner.ra;
                    if ((ra.abs() < ratol)
                        || (ra - DPIBY2).abs() < ratol
                        || (ra - DPI).abs() < ratol
                        || (ra - 3.0 * DPIBY2).abs() < ratol
                        || (ra - 2.0 * DPI).abs() < ratol)
                        && ((corner.dec - THETAX).abs() < dectol
                            || (corner.dec + THETAX).abs() < dectol)
                    {
                        corner.interior = true;
                    }
                } else if corner.cells.len() == 4 {
                    corner.interior = true;
                }
            }
            if isolated && !self.corners[id].interior {
                let cells = self.corners[id].cells.clone();
                if cells.len() < 2 {
                    self.corners[id].isolated = true;
                } else if cells.len() == 2 {
                    let (a, b) = (self.cells[cells[0]].corners, self.cells[cells[1]].corners);
                    // bl, tl, tr, br
                    let edge = a[1] == b[0] || a[2] == b[1] || a[3] == b[2] || a[0] == b[3];
                    if !edge {
                        self.corners[id].isolated = true;
                    }
                }
            }
        }
        if isolated {
            for &c in chain {
                let k = self.cells[c].corners;
                if k.iter().all(|&id| self.corners[id].isolated) {
                    self.cells[c].isolated = true;
                }
            }
        }
        Ok(list)
    }

    /// moc.c `AppendChildren`: the four cells at `order + 1` into which
    /// `cell` divides, appended to `chain`, interior if it is.
    fn append_children(&mut self, cell: usize, chain: &mut Vec<usize>) {
        let gx0 = 2 * self.cells[cell].ix - 1;
        let gy0 = 2 * self.cells[cell].iy - 1;
        let interior = self.cells[cell].interior;
        for iy in 0..2 {
            for ix in 0..2 {
                let id = self.make_cell(gx0 + ix, gy0 + iy, chain);
                self.cells[id].interior = interior;
            }
        }
    }
}

impl Moc {
    /// moc.c `RegBaseMesh`: a point at every corner of a normalised cell
    /// that lies on the boundary, in ICRS, sorted by Dec then RA, all RA in
    /// [0, 2PI[. An empty Moc, or one of the whole sky, has no boundary and
    /// gives one bad point. MeshSize is not used.
    fn base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        if let Some(mesh) = &self.cache.borrow().basemesh {
            return Ok(mesh.clone());
        }
        let maxorder_attr = self.max_order();
        let full_sky = self.nrange() == 1
            && self.range[0] == 0
            && self.range[1] == 12 * (1_i64 << (2 * maxorder_attr)) - 1;
        let mesh = if self.nrange() == 0 || full_sky {
            vec![vec![BAD_VALUE, BAD_VALUE]]
        } else {
            self.trace_mesh()?
        };
        self.cache.borrow_mut().basemesh = Some(mesh.clone());
        Ok(mesh)
    }

    fn trace_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        let tol = 0.1 * DD2R * order_to_res(self.max_order()) / 3600.0;
        let mut mesh = Mesh {
            moc: self,
            tol,
            cells: Vec::new(),
            corners: Vec::new(),
        };
        let mut chains: Vec<Vec<usize>> = vec![Vec::new(); (MOC_MAX_ORDER + 2) as usize];
        let norm = self.with_norm("astRegBaseMesh", |norm, _| norm.to_vec())?;
        let mut minorder = -1;
        let mut maxorder = -1;
        for nuniq in norm {
            let (order, npix) = decode_nuniq(nuniq);
            let (ix, iy) = nested_to_xy(npix, order);
            let mut chain = std::mem::take(&mut chains[order as usize]);
            mesh.make_cell(ix, iy, &mut chain);
            chains[order as usize] = chain;
            if minorder == -1 {
                minorder = order;
            }
            maxorder = order;
        }

        for order in minorder..maxorder {
            let o = order as usize;
            if chains[o].is_empty() {
                continue;
            }
            let chain = std::mem::take(&mut chains[o]);
            mesh.make_corners(order, &chain, false, false)?;
            for &c in chain.iter().rev() {
                if mesh.cells[c]
                    .corners
                    .iter()
                    .all(|&k| mesh.corners[k].interior)
                {
                    mesh.cells[c].interior = true;
                }
            }
            let mut next = std::mem::take(&mut chains[o + 1]);
            for &c in chain.iter().rev() {
                let core = mesh.cells[c].corners.iter().all(|&k| {
                    mesh.corners[k]
                        .cells
                        .iter()
                        .all(|&n| mesh.cells[n].interior)
                });
                if core {
                    if !mesh.cells[c].interior {
                        return Err(AstError::new(
                            ErrorCode::Inter,
                            "astRegBaseMesh(Moc): Core cell not flagged as interior (internal \
                             programming error).",
                        ));
                    }
                } else {
                    mesh.append_children(c, &mut next);
                }
            }
            chains[o + 1] = next;
        }

        let chain = std::mem::take(&mut chains[maxorder as usize]);
        let corners = mesh.make_corners(maxorder, &chain, true, true)?;
        Ok(corners
            .iter()
            .filter(|&&k| !mesh.corners[k].interior)
            .map(|&k| vec![mesh.corners[k].ra, mesh.corners[k].dec])
            .collect())
    }
}

// ============================================================================
// MOC text and data
// ============================================================================

/// The NUNIQ values of a normalised Moc, as a FITS binary table holds them:
/// 4 byte values while every cell is below order 14, otherwise 8 byte ones
/// (`MocType`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MocData {
    /// 4 byte values (`MocType` 4).
    Int32(Vec<i32>),
    /// 8 byte values (`MocType` 8).
    Int64(Vec<i64>),
}

/// The highest HEALPix order a Moc can use (`AST__MXORDHPX`).
const MXORDHPX: i32 = MOC_MAX_ORDER;

/// The largest value that can have another decimal digit appended without
/// overflowing an `i64` (moc.c `MXVALUE`).
const MXVALUE: i64 = (i64::MAX - 9) / 10;

/// C's `isspace` in the "C" locale.
fn c_isspace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// At most `n` bytes of `text` from `at`, stopping at a null, as C's
/// `%.<n>s` prints them.
fn snippet(text: &[u8], at: usize, n: usize) -> String {
    let rest = text.get(at..).unwrap_or(&[]);
    let end = rest
        .iter()
        .take(n)
        .position(|&c| c == 0)
        .unwrap_or(rest.len().min(n));
    String::from_utf8_lossy(&rest[..end]).into_owned()
}

impl Moc {
    /// Combine a MOC held as NUNIQ values, as a FITS binary table holds one,
    /// with the Moc (C's `astAddMocData`). `cmode` combines them as
    /// `add_region` does; `negate` uses the cells the data do not hold.
    /// `maxorder` is the order to use if MaxOrder is not set, which it then
    /// becomes; if it is negative the highest order in the data is used.
    /// Cells above that order are degraded to it. A value below 4, which
    /// encodes no cell, is AST__INMOC, and leaves the Moc unchanged.
    pub fn add_moc_data<T: Copy + Into<i64>>(
        &mut self,
        cmode: CmpOperator,
        negate: bool,
        maxorder: i32,
        data: &[T],
    ) -> AstResult<()> {
        let method = "astAddMocData";
        if maxorder > MXORDHPX {
            return Err(AstError::new(
                ErrorCode::Invar,
                format!(
                    "{method}(Moc): Invalid value ({maxorder}) supplied for parameter \
                     'maxorder' - must be no greater than {MXORDHPX}."
                ),
            ));
        }
        if data.is_empty() {
            if cmode == CmpOperator::And {
                self.range.clear();
                self.clear_cache();
            }
            return Ok(());
        }
        if let Some((icell, nuniq)) = data
            .iter()
            .map(|&v| v.into())
            .enumerate()
            .find(|&(_, v)| v < 4)
        {
            return Err(AstError::new(
                ErrorCode::Inmoc,
                format!(
                    "{method}(Moc): Invalid NUNIQ value ({nuniq}) at element {icell} of the \
                     supplied data - must be at least 4."
                ),
            ));
        }
        let maxorder = if self.test_max_order() {
            self.max_order()
        } else {
            let mut order = maxorder;
            if order < 0 {
                for &v in data {
                    order = order.max(decode_nuniq(v.into()).0);
                }
            }
            self.store_max_order(order)?;
            order
        };
        let nold = self.nrange();
        for &v in data {
            let (order, npix) = decode_nuniq(v.into());
            let shift = 2 * (maxorder - order);
            if shift > 0 {
                self.range
                    .extend([npix << shift, ((npix + 1) << shift) - 1]);
            } else {
                self.range.extend([npix >> -shift, npix >> -shift]);
            }
        }
        self.moc_norm(negate, cmode, nold, maxorder, method)
    }

    /// The NUNIQ values of the normalised Moc, for a FITS binary table (C's
    /// `astGetMocData`), with the header `get_moc_header` gives.
    pub fn get_moc_data(&self) -> AstResult<MocData> {
        self.with_norm("astGetMocData", |norm, is32| {
            if is32 && !norm.is_empty() {
                MocData::Int32(norm.iter().map(|&v| v as i32).collect())
            } else {
                MocData::Int64(norm.to_vec())
            }
        })
    }

    /// The headers of a FITS binary table holding the Moc's NUNIQ values
    /// (C's `astGetMocHeader`), as the IVOA MOC recommendation describes it.
    pub fn get_moc_header(&self) -> AstResult<FitsChan> {
        let mtype = self.moc_type()?;
        let moclen = self.moc_length()?;
        let maxorder = self.max_order();
        let mut fc = FitsChan::new();
        fc.set_fits_s(
            "XTENSION",
            "BINTABLE",
            "HEALPix Multi Order Coverage map",
            true,
        );
        fc.set_fits_i("BITPIX", 8, "", true);
        fc.set_fits_i("NAXIS", 2, "", true);
        fc.set_fits_i("NAXIS1", i64::from(mtype), "Bytes per row", true);
        fc.set_fits_i("NAXIS2", moclen as i64, "No. of rows", true);
        fc.set_fits_i("PCOUNT", 0, "", true);
        fc.set_fits_i("GCOUNT", 1, "", true);
        fc.set_fits_i("TFIELDS", 1, "", true);
        fc.set_fits_s("TFORM1", if mtype == 4 { "1J" } else { "1K" }, "", true);
        fc.set_fits_s("TTYPE1", "UNIQ", "HEALPix UNIQ pixel number", true);
        fc.set_fits_s("PIXTYPE", "HEALPIX", "HEALPix magic code", true);
        fc.set_fits_s("ORDERING", "NUNIQ", "NUNIQ coding method", true);
        fc.set_fits_s("COORDSYS", "C", "ICRS coordinates", true);
        fc.set_fits_i(
            "MOCORDER",
            i64::from(maxorder),
            "MOC resolution (best order)",
            true,
        );
        fc.set_fits_s("MOCTOOL", PACKAGE_STRING, "Name of the MOC generator", true);
        Ok(fc)
    }

    /// Combine a MOC in the string or JSON serialisation of the IVOA MOC
    /// recommendation with the Moc (C's `astAddMocString`), returning whether
    /// it was JSON, which a first non-blank `{` selects. `cmode`, `negate`
    /// and `maxorder` are as for `add_moc_data`; cells above `maxorder` are
    /// degraded to it, as C's code does, although its documentation says
    /// they are ignored. The text ends at a null character if it has one.
    /// Malformed text is AST__INMOC, and leaves the Moc unchanged.
    pub fn add_moc_string(
        &mut self,
        cmode: CmpOperator,
        negate: bool,
        maxorder: i32,
        text: &str,
    ) -> AstResult<bool> {
        let method = "astAddMocString";
        if maxorder > MXORDHPX {
            return Err(AstError::new(
                ErrorCode::Invar,
                format!(
                    "{method}(Moc): Invalid value ({maxorder}) supplied for parameter \
                     'maxorder' - must be no greater than {MXORDHPX}."
                ),
            ));
        }
        if text.is_empty() {
            if cmode == CmpOperator::And {
                self.range.clear();
                self.clear_cache();
            }
            return Ok(false);
        }
        let nold = self.nrange();
        let json = self.add_moc_text(maxorder, [text.as_bytes()], method)?;
        let maxorder = self.max_order();
        self.moc_norm(negate, cmode, nold, maxorder, method)?;
        Ok(json)
    }

    /// The Moc in the string or JSON serialisation of the IVOA MOC
    /// recommendation (C's `astGetMocString`).
    pub fn get_moc_string(&self, json: bool) -> AstResult<String> {
        let mut out = String::new();
        self.get_moc_text(
            json,
            80,
            &mut |chunk| {
                out.push_str(chunk);
                Ok(())
            },
            "astGetMocString",
        )?;
        Ok(out)
    }

    /// moc.c `astAddMocText`: append to the Moc's ranges the cells of a MOC
    /// in the string or JSON serialisation, read from the successive
    /// `chunks` of its text, returning whether it was JSON. The ranges are
    /// at MaxOrder, which, if not set, becomes `maxorder` or, if that is
    /// negative, the highest order in the text. They are not normalised.
    pub(crate) fn add_moc_text<'a>(
        &mut self,
        maxorder: i32,
        chunks: impl IntoIterator<Item = &'a [u8]>,
        method: &str,
    ) -> AstResult<bool> {
        if maxorder > MXORDHPX {
            return Err(AstError::new(
                ErrorCode::Invar,
                format!(
                    "{method}(Moc): Invalid value ({maxorder}) supplied for parameter \
                     'maxorder' - must be no greater than {MXORDHPX}."
                ),
            ));
        }
        let bad = |kind: &str, text: &[u8], detail: String| {
            AstError::new(
                ErrorCode::Inmoc,
                format!("{method}(Moc): {kind}: '{}...'", snippet(text, 0, 30)),
            )
            .with_context(detail)
        };
        let invalid_json =
            |text: &[u8], detail: String| bad("Invalid JSON MOC supplied", text, detail);
        let invalid_string =
            |text: &[u8], detail: String| bad("Invalid string MOC supplied", text, detail);

        let mut orders: Vec<Vec<i64>> = vec![Vec::new(); (MXORDHPX + 1) as usize];
        let mut mxord = -1;
        let mut order: i32 = -1;
        let mut npix: i64 = 0;
        let mut isrange = false;
        let mut npix0: i64 = 0;
        let mut state = 0;
        let mut json = false;
        let mut first = false;

        for text in chunks {
            let mut pt = 0;
            while pt < text.len() && text[pt] != 0 {
                let c = text[pt];
                let at = || snippet(text, pt, 15);
                if state == 0 {
                    if c == b'{' {
                        json = true;
                        state = 1;
                    } else if c.is_ascii_digit() {
                        // Read the digit again as the start of the first value.
                        json = false;
                        state = 1;
                        continue;
                    } else if !c_isspace(c) {
                        return Err(AstError::new(
                            ErrorCode::Inmoc,
                            format!(
                                "{method}(Moc): Invalid textual MOC supplied: '{}...'",
                                snippet(text, pt, 30)
                            ),
                        )
                        .with_context(
                            "Cannot determine the serialisation from the first character.",
                        ));
                    }
                } else if json {
                    match state {
                        1 => {
                            if c == b'"' {
                                order = 0;
                                state = 2;
                            } else if c == b'}' && order < 0 {
                                state = 9;
                            } else if !c_isspace(c) {
                                return Err(invalid_json(
                                    text,
                                    format!("Expected double quote at '{}'.", at()),
                                ));
                            }
                        }
                        2 => {
                            if c.is_ascii_digit() {
                                order = i32::from(c - b'0') + 10 * order;
                                if order > MXORDHPX {
                                    return Err(bad(
                                        "Error reading JSON MOC",
                                        text,
                                        format!("Invalid MOC order {order} encountrered."),
                                    ));
                                }
                            } else if c == b'"' {
                                state = 3;
                            } else {
                                return Err(invalid_json(
                                    text,
                                    format!("Expected double quote or digit at '{}'.", at()),
                                ));
                            }
                        }
                        3 => {
                            if c == b':' {
                                state = 4;
                            } else if !c_isspace(c) {
                                return Err(invalid_json(
                                    text,
                                    format!("Expected colon at '{}'.", at()),
                                ));
                            }
                        }
                        4 => {
                            if c == b'[' {
                                first = true;
                                state = 5;
                            } else if !c_isspace(c) {
                                return Err(invalid_json(
                                    text,
                                    format!("Expected opening square bracket at '{}'.", at()),
                                ));
                            }
                        }
                        5 => {
                            if c.is_ascii_digit() {
                                state = 6;
                                npix = i64::from(c - b'0');
                            } else if first && c == b']' {
                                mxord = mxord.max(order);
                                state = 8;
                            } else if !c_isspace(c) {
                                return Err(invalid_json(
                                    text,
                                    format!("Expected digit at '{}'.", at()),
                                ));
                            }
                        }
                        6 => {
                            first = false;
                            if c.is_ascii_digit() {
                                if npix > MXVALUE {
                                    return Err(invalid_json(
                                        text,
                                        format!("Value too large at '{}'.", at()),
                                    ));
                                }
                                npix = i64::from(c - b'0') + 10 * npix;
                            } else if c_isspace(c) {
                                state = 7;
                            } else if c == b',' {
                                state = 5;
                            } else if c == b']' {
                                state = 8;
                            } else {
                                return Err(invalid_json(
                                    text,
                                    format!(
                                        "Expected digit, comma or closing square bracket at '{}'.",
                                        at()
                                    ),
                                ));
                            }
                            if state != 6 {
                                let list = &mut orders[order as usize];
                                if list.is_empty() {
                                    mxord = mxord.max(order);
                                }
                                list.push(npix);
                            }
                        }
                        7 => {
                            if c == b',' {
                                state = 5;
                            } else if c == b']' {
                                state = 8;
                            } else if !c_isspace(c) {
                                return Err(invalid_json(
                                    text,
                                    format!(
                                        "Expected comma or closing square bracket at '{}'.",
                                        at()
                                    ),
                                ));
                            }
                        }
                        8 => {
                            if c == b',' {
                                state = 1;
                            } else if c == b'}' {
                                state = 9;
                            } else if !c_isspace(c) {
                                return Err(invalid_json(
                                    text,
                                    format!("Expected comma or closing curly brace at '{}'.", at()),
                                ));
                            }
                        }
                        _ => {
                            if !c_isspace(c) {
                                return Err(invalid_json(
                                    text,
                                    format!(
                                        "Illegal non-blank characters following final closing curly brace at '{}'.",
                                        at()
                                    ),
                                ));
                            }
                        }
                    }
                } else if state == 1 {
                    if c.is_ascii_digit() {
                        npix = i64::from(c - b'0');
                        state = 2;
                    } else if !c_isspace(c) {
                        return Err(invalid_string(
                            text,
                            format!("Expected digit at '{}'.", at()),
                        ));
                    }
                } else if c.is_ascii_digit() {
                    if npix > MXVALUE {
                        return Err(invalid_string(
                            text,
                            format!("Value too large at '{}'.", at()),
                        ));
                    }
                    npix = i64::from(c - b'0') + 10 * npix;
                } else if c_isspace(c) || c == b',' {
                    if order < 0 {
                        return Err(invalid_string(
                            text,
                            "No order value found at start of string.".to_string(),
                        ));
                    }
                    if !isrange {
                        npix0 = npix;
                    } else if npix >= npix0 {
                        isrange = false;
                    } else {
                        return Err(invalid_string(
                            text,
                            format!("Range start ({npix0}) is after range end ({npix})."),
                        ));
                    }
                    orders[order as usize].extend(npix0..=npix);
                    state = 1;
                } else if c == b'/' {
                    if npix > i64::from(MXORDHPX) {
                        return Err(bad(
                            "Error reading string MOC",
                            text,
                            format!("Invalid MOC order {npix} encountrered."),
                        ));
                    }
                    order = npix as i32;
                    mxord = mxord.max(order);
                    state = 1;
                } else if c == b'-' {
                    isrange = true;
                    npix0 = npix;
                    state = 1;
                } else {
                    return Err(invalid_string(
                        text,
                        format!("Illegal character at '{}'.", at()),
                    ));
                }
                pt += 1;
            }
        }

        // After the last chunk C's text pointer is null, so messages quote
        // nothing.
        let end: &[u8] = &[];
        if state == 0 {
            return Err(AstError::new(
                ErrorCode::Inmoc,
                format!("{method}(Moc): Blank MOC text supplied."),
            ));
        } else if json {
            if state != 9 {
                return Err(invalid_json(
                    end,
                    "No closing curly brace found.".to_string(),
                ));
            }
        } else if state == 2 {
            if order < 0 {
                return Err(invalid_string(
                    end,
                    "No order value found at start of string.".to_string(),
                ));
            } else if isrange && npix < npix0 {
                return Err(invalid_string(
                    end,
                    format!("Range start ({npix0}) is after range end ({npix})."),
                ));
            }
            if !isrange {
                npix0 = npix;
            }
            orders[order as usize].extend(npix0..=npix);
        }

        for (o, values) in orders.iter().enumerate() {
            let ncell = 12 * (1_i64 << (2 * o));
            if let Some(&v) = values.iter().find(|&&v| v >= ncell) {
                return Err(AstError::new(
                    ErrorCode::Inmoc,
                    format!(
                        "{method}(Moc): Invalid NPIX value ({v}) for order {o} in the supplied \
                         MOC - there are {ncell} cells at that order."
                    ),
                ));
            }
        }

        let maxorder = if self.test_max_order() {
            self.max_order()
        } else {
            let order = if maxorder >= 0 {
                maxorder
            } else if mxord >= 0 {
                mxord
            } else {
                return Err(AstError::new(
                    ErrorCode::Inmoc,
                    format!("{method}(Moc): Failed to read string MOC."),
                )
                .with_context("No order value found."));
            };
            self.store_max_order(order)?;
            order
        };

        for (o, values) in orders
            .iter()
            .enumerate()
            .take((maxorder.max(mxord) + 1) as usize)
        {
            let shift = 2 * (maxorder - o as i32);
            for &v in values {
                if shift > 0 {
                    self.range.extend([v << shift, ((v + 1) << shift) - 1]);
                } else {
                    self.range.extend([v >> -shift, v >> -shift]);
                }
            }
        }
        Ok(json)
    }

    /// moc.c `astGetMocText`: write the Moc in the string or JSON
    /// serialisation, passing it to `sink` in pieces of at most `buflen`
    /// bytes, each but the last ending after a space or comma; AST__SMBUF
    /// if a value does not fit in what is left of the buffer after the last
    /// space or comma.
    pub fn get_moc_text(
        &self,
        json: bool,
        buflen: usize,
        sink: &mut dyn FnMut(&str) -> AstResult<()>,
        method: &str,
    ) -> AstResult<()> {
        let norm = self.with_norm(method, |norm, _| norm.to_vec())?;
        let mut buf = vec![0_u8; buflen];
        let mut pw = 0_usize;
        let too_small = || {
            AstError::new(
                ErrorCode::Smbuf,
                format!("{method}(Moc): Supplied buffer length ({buflen}) is too small."),
            )
        };
        let flush = |sink: &mut dyn FnMut(&str) -> AstResult<()>, bytes: &[u8]| {
            sink(std::str::from_utf8(bytes).expect("MOC text is ASCII"))
        };
        // moc.c TOKEN_WRITE: append a token, and when the buffer fills,
        // pass on everything up to its last space or comma.
        let write = |token: &str,
                     buf: &mut Vec<u8>,
                     pw: &mut usize,
                     sink: &mut dyn FnMut(&str) -> AstResult<()>|
         -> AstResult<()> {
            let tok = token.as_bytes();
            let mut k = 0;
            while *pw < buflen && k < tok.len() {
                buf[*pw] = tok[k];
                *pw += 1;
                k += 1;
            }
            if *pw == buflen && k < tok.len() {
                let Some(pc) = buf[..*pw].iter().rposition(|&c| c == b' ' || c == b',') else {
                    return Err(too_small());
                };
                flush(sink, &buf[..=pc])?;
                buf.copy_within(pc + 1..*pw, 0);
                *pw -= pc + 1;
                while *pw < buflen && k < tok.len() {
                    buf[*pw] = tok[k];
                    *pw += 1;
                    k += 1;
                }
                if k < tok.len() {
                    return Err(too_small());
                }
            }
            Ok(())
        };

        let mut first = true;
        let mut order = -1;
        let mut npix_start = 0_i64;
        let mut npix_prev = 0_i64;
        let mut npix = 0_i64;
        for &nuniq in &norm {
            let (neworder, n) = decode_nuniq(nuniq);
            npix = n;
            if json {
                if neworder != order {
                    order = neworder;
                    let token = if first {
                        format!("{{\"{order}\":[{npix}")
                    } else {
                        format!("],\"{order}\":[{npix}")
                    };
                    first = false;
                    write(&token, &mut buf, &mut pw, sink)?;
                } else {
                    write(&format!(",{npix}"), &mut buf, &mut pw, sink)?;
                }
            } else {
                if neworder != order {
                    if npix_start < npix_prev {
                        write(&format!("-{npix_prev}"), &mut buf, &mut pw, sink)?;
                    }
                    order = neworder;
                    npix_start = npix;
                    let token = if first {
                        format!("{order}/{npix}")
                    } else {
                        format!(" {order}/{npix}")
                    };
                    first = false;
                    write(&token, &mut buf, &mut pw, sink)?;
                } else if npix > npix_prev + 1 {
                    if npix_start < npix_prev {
                        write(&format!("-{npix_prev}"), &mut buf, &mut pw, sink)?;
                    }
                    write(&format!(",{npix}"), &mut buf, &mut pw, sink)?;
                    npix_start = npix;
                }
                npix_prev = npix;
            }
        }
        if !first {
            if json {
                write("]", &mut buf, &mut pw, sink)?;
            } else if npix_start < npix {
                write(&format!("-{npix}"), &mut buf, &mut pw, sink)?;
            }
        }
        let maxorder = self.max_order();
        if order < maxorder {
            let token = match (json, first) {
                (true, true) => format!("{{\"{maxorder}\":[]"),
                (true, false) => format!(",\"{maxorder}\":[]"),
                (false, true) => format!("{maxorder}/"),
                (false, false) => format!(" {maxorder}/"),
            };
            first = false;
            write(&token, &mut buf, &mut pw, sink)?;
        }
        if json {
            write(if first { "{}" } else { "}" }, &mut buf, &mut pw, sink)?;
        }
        if pw > 0 {
            flush(sink, &buf[..pw])?;
        }
        Ok(())
    }
}

// ============================================================================
// Adding Regions and pixel masks
// ============================================================================

/// The cells found inside a region of interest at each order, as grid
/// coordinates in one HPX-like projection, with the Mapping from that
/// projection to HPX12 at each order (moc.c `CellList`).
struct CellList {
    maxorder: i32,
    maps: Vec<Option<DynMapping>>,
    ix: Vec<Vec<i64>>,
    iy: Vec<Vec<i64>>,
}

/// The positions, in the coordinates of the region of interest, that lie
/// in it.
type InsideTest<'a> = dyn Fn(&PointSet) -> AstResult<Vec<bool>> + 'a;

/// The indices of the sky axes in `frame`, the first the longitude axis of
/// a SkyFrame and the second its latitude (moc.c `FindSkyAxes`).
fn sky_axes(frame: &dyn Frame, method: &str, class: &str) -> AstResult<[usize; 2]> {
    let mut skyaxes = [None, None];
    for iax in 0..frame.naxes() {
        let (frm, pax) = frame.primary_frame(iax);
        if frm.as_any().is::<SkyFrame>() {
            skyaxes[usize::from(pax != 0)] = Some(iax);
        }
    }
    match skyaxes {
        [Some(a), Some(b)] => Ok([a, b]),
        _ => Err(AstError::new(
            ErrorCode::Nosky,
            format!(
                "{method}(Moc): The supplied {class} does not contain a pair of celestial \
                 coordinate axes."
            ),
        )),
    }
}

/// moc.c `FindSkyAxes` for a Region: the Region its sky axes span
/// (AST__BDSKY when those axes do not form a Region).
fn find_sky_axes_region(region: &dyn Region, method: &str) -> AstResult<DynRegion> {
    let class = super::class_name(region.as_any());
    let axes = sky_axes(region.get_region_frame().as_ref(), method, class)?;
    match super::pick_axes_region(region, &axes)? {
        super::PickedAxes::Region(r) => Ok(r),
        super::PickedAxes::Frame(_) => Err(AstError::new(
            ErrorCode::Bdsky,
            format!(
                "{method}(Moc): The region of sky described by the supplied {class} cannot be \
                 determined."
            ),
        )),
    }
}

/// moc.c `FindSkyAxes` for a FrameSet: a FrameSet whose current Frame holds
/// the sky axes of `wcs` and whose base Frame the base axes they depend on,
/// the base axes picked by the sky axes' indices as C picks them (AST__BDSKY
/// when the sky axes cannot be split from the others).
fn find_sky_axes_frameset(wcs: &FrameSet, method: &str) -> AstResult<FrameSet> {
    let axes = sky_axes(wcs.current(), method, "FrameSet")?;
    let one_based = [axes[0] + 1, axes[1] + 1];
    let mut inverted = wcs.clone();
    inverted.invert();
    match inverted.map_split(&one_based)? {
        Some((outs, mut map)) if map.nout() == 2 => {
            let (bfrm, _) = crate::frames::pick_axes(wcs.base(), &one_based)?;
            let mut result = FrameSet::new(bfrm);
            map.invert();
            let perm: Vec<i64> = outs.iter().map(|&a| a as i64 - 1).collect();
            if perm[0] != 0 {
                let pm = crate::mappings::PermMap::new(&perm, &perm, &[])?;
                map = Box::new(CmpMap::new(Box::new(pm), map, Composition::Series)?);
            }
            let (frm, _) = crate::frames::pick_axes(wcs.current(), &one_based)?;
            result.add_frame(FrameRef::Base, map, frm)?;
            Ok(result)
        }
        _ => Err(AstError::new(
            ErrorCode::Bdsky,
            format!(
                "{method}(Moc): The sky axes in the supplied WCS FrameSet cannot be split from \
                 the other axes."
            ),
        )),
    }
}

/// moc.c `GetPixelArea`: the area, in square arc-seconds, of the pixel at
/// the centre of an array of `dims` pixels, from the distances to its
/// neighbours along each grid axis; bad if either distance is.
fn get_pixel_area(wcs: &FrameSet, dims: [usize; 2]) -> AstResult<f64> {
    let x0 = 0.5 * (dims[0] as f64 + 1.0);
    let y0 = 0.5 * (dims[1] as f64 + 1.0);
    let pts = PointSet::from_rows(
        [[x0, y0], [x0 + 1.0, y0], [x0, y0 + 1.0]]
            .iter()
            .map(|p| p.as_slice()),
    );
    let sky = wcs.transform(&pts, true)?;
    let d1 = Frame::distance(wcs, sky.point(0), sky.point(1))?;
    let d2 = Frame::distance(wcs, sky.point(0), sky.point(2))?;
    if is_bad(d1) || is_bad(d2) {
        return Ok(BAD_VALUE);
    }
    let scale = (180.0 / DPI) * 3600.0;
    Ok((d1 * scale) * (d2 * scale))
}

/// The Mapping from grid coordinates at each order from `minorder` to
/// `maxorder` in an HPX-like projection to the coordinates `top` gives at
/// `maxorder`: each lower order's grid divided into four of the next, by a
/// WinMap ahead of the Mapping for the order above (the loop moc.c
/// `AddRegion` and `AddPixelMask<X>` share).
fn order_maps(top: DynMapping, maxorder: i32, minorder: i32) -> AstResult<Vec<Option<DynMapping>>> {
    let incmap =
        crate::mappings::WinMap::from_corners(&[0.5, 0.5], &[1.5, 1.5], &[0.5, 0.5], &[2.5, 2.5])?;
    let mut maps: Vec<Option<DynMapping>> = vec![None; (maxorder + 1) as usize];
    let mut map = top;
    for i in (0..=maxorder).rev() {
        if i >= minorder {
            maps[i as usize] = Some(map.simplify()?);
            map = Box::new(CmpMap::new(
                Box::new(incmap.clone()),
                map,
                Composition::Series,
            )?);
        }
    }
    Ok(maps)
}

/// The grid bounds at `maxorder`, `[lx, ly, ux, uy]`, taken to `minorder`
/// with a one cell margin (the step moc.c `AddRegion` and
/// `AddPixelMask<X>` share).
fn bounds_at_minorder(
    maps: &[Option<DynMapping>],
    maxorder: i32,
    minorder: i32,
    b: [i64; 4],
) -> AstResult<[i64; 4]> {
    let top = maps[maxorder as usize]
        .as_ref()
        .expect("map at maxorder")
        .clone_box();
    let mut low = maps[minorder as usize]
        .as_ref()
        .expect("map at minorder")
        .clone_box();
    low.invert();
    let map: DynMapping = Box::new(CmpMap::new(top, low, Composition::Series)?);
    let map = map.simplify()?;
    let pts = PointSet::from_rows(
        [[b[0] as f64, b[1] as f64], [b[2] as f64, b[3] as f64]]
            .iter()
            .map(|p| p.as_slice()),
    );
    let out = map.transform(&pts, true)?;
    Ok([
        out.point(0)[0].round() as i64 - 1,
        out.point(0)[1].round() as i64 - 1,
        out.point(1)[0].round() as i64 + 1,
        out.point(1)[1].round() as i64 + 1,
    ])
}

impl Moc {
    /// Combine a Region with the Moc (C's `astAddRegion`): AND keeps what
    /// both cover, OR what either covers, and XOR what one but not both
    /// covers. The Moc's own Negated is ignored. A Moc is combined cell for
    /// cell. Any other Region must have a pair of sky axes (AST__NOSKY);
    /// cells at `MinOrder` over its bounding box in the HPX-like projection
    /// where it is smallest are tested at nine points, those wholly inside
    /// kept, those partly inside divided, down to `MaxOrder`, where a cell is
    /// in if its centre is. MaxOrder, if not set, becomes the order whose
    /// cells are about 0.1% of the Region's size.
    pub fn add_region(&mut self, cmode: CmpOperator, region: &dyn Region) -> AstResult<()> {
        if let Some(that) = region.as_any().downcast_ref::<Moc>() {
            return self.add_moc(cmode, that);
        }
        let method = "astAddRegion";
        let mut picked = find_sky_axes_region(region, method)?;
        let negated = picked.is_negated();
        picked.clear_negated();

        let mut maxorder = if self.test_max_order() {
            self.max_order()
        } else {
            let (_, radius) = picked.get_region_disc()?;
            let order = res_to_order(0.001 * radius * (180.0 / DPI) * 3600.0);
            self.store_max_order(order)?;
            order
        };
        let mut minorder = self.min_order();
        let mut oversample = 0;
        if minorder >= maxorder {
            oversample = minorder - maxorder;
            maxorder = minorder;
            minorder = maxorder - 1;
        }
        if minorder < 0 {
            return Err(AstError::new(
                ErrorCode::Invar,
                format!(
                    "{method}(Moc): Invalid value ({minorder}) supplied for parameter 'MinOrder'."
                ),
            ));
        }

        let picked_frame = picked.get_region_frame();
        let mut fs = hpx12_frameset(maxorder)?;
        let gridframe = fs.base().snapshot();
        fs.invert();
        let Some(fsconv) = picked_frame.convert_to(&fs, &[])? else {
            return Err(AstError::new(
                ErrorCode::Nodef,
                format!(
                    "{method}(Moc): The supplied {} cannot be mapped to HEALPix coordinates.",
                    super::class_name(region.as_any())
                ),
            ));
        };
        let reg2hpx12 = fsconv.mapping(FrameRef::Base, FrameRef::Current)?;

        // The HPX-like projection in which the Region's bounding box covers
        // the fewest cells.
        let mut best: Option<(usize, [i64; 4], DynRegion)> = None;
        let mut npix_min = 0_usize;
        for (iproj, proj) in XPH_PROJECTIONS.iter().enumerate() {
            let mut xph = XphMap::new(maxorder as u32, *proj);
            xph.invert();
            let reg2proj = CmpMap::new(reg2hpx12.clone_box(), Box::new(xph), Composition::Series)?;
            let mapped = match picked.reg_map_region(&reg2proj, gridframe.as_ref()) {
                Ok(m) => m,
                Err(e) if e.code() == ErrorCode::Nodef => continue,
                Err(e) => return Err(e),
            };
            let (glbnd, gubnd) = mapped.get_region_bounds()?;
            let npix =
                (((gubnd[0] - glbnd[0] + 1.0) * (gubnd[1] - glbnd[1] + 1.0)) as i32) as usize;
            if iproj == 0 || npix < npix_min {
                let d0 = 0.05 * (gubnd[0] - glbnd[0] + 1.0);
                let d1 = 0.05 * (gubnd[1] - glbnd[1] + 1.0);
                let b = [
                    (glbnd[0] + HALF - d0) as i64,
                    (glbnd[1] + HALF - d1) as i64,
                    (gubnd[0] + HALF + d0) as i64,
                    (gubnd[1] + HALF + d1) as i64,
                ];
                npix_min = npix;
                best = Some((iproj, b, mapped));
            }
        }
        let Some((iproj_min, b, mapped_min)) = best else {
            return Err(AstError::new(
                ErrorCode::Nodef,
                format!(
                    "{method}(Moc): The supplied {} cannot be mapped to HEALPix coordinates.",
                    super::class_name(region.as_any())
                ),
            ));
        };

        let maps = order_maps(Box::new(UnitMap::new(2)), maxorder, minorder)?;
        let b = bounds_at_minorder(&maps, maxorder, minorder, b)?;
        let test = |pts: &PointSet| -> AstResult<Vec<bool>> {
            let out = mapped_min.transform(pts, true)?;
            Ok((0..out.npoint())
                .map(|i| !is_bad(out.point(i)[0]))
                .collect())
        };
        self.fill_cells(
            &maps, maxorder, minorder, oversample, iproj_min, b, &test, negated, cmode, method,
        )
    }

    /// moc.c `AddRegion` for a Moc: its ranges, taken to this Moc's
    /// MaxOrder, combined with this Moc's, negated if it is.
    fn add_moc(&mut self, cmode: CmpOperator, that: &Moc) -> AstResult<()> {
        if that.range.is_empty() {
            if cmode == CmpOperator::And {
                self.range.clear();
                self.clear_cache();
            }
            return Ok(());
        }
        let that_order = that.max_order();
        let maxorder = if self.test_max_order() {
            self.max_order()
        } else {
            self.store_max_order(that_order)?;
            that_order
        };
        let nold = self.nrange();
        let shift = 2 * (maxorder - that_order);
        for r in that.range.chunks(2) {
            let (ilow, ihigh) = if shift > 0 {
                (r[0] << shift, ((r[1] + 1) << shift) - 1)
            } else {
                (r[0] >> -shift, r[1] >> -shift)
            };
            self.range.extend([ilow, ihigh]);
        }
        self.moc_norm(that.is_negated(), cmode, nold, maxorder, "astAddRegion")
    }

    /// Combine the pixels of a 2-dimensional array that pass a test with the
    /// Moc (C's `astAddPixelMask<X>`): those whose value compares with
    /// `value` as `oper` says, and, if `bad` is given, whose value is not
    /// `bad`. `wcs` takes grid coordinates in the array to coordinates with
    /// a pair of sky axes (AST__NOSKY); `dims` are the array's dimensions,
    /// its first axis varying fastest. MaxOrder, if not set, becomes the
    /// lowest order whose cells are no larger than a quarter of the pixel at
    /// the array's centre. The cells are found as `add_region` finds them.
    #[allow(clippy::too_many_arguments)]
    pub fn add_pixel_mask<T: PixelValue>(
        &mut self,
        cmode: CmpOperator,
        wcs: &FrameSet,
        value: T,
        oper: PixelOper,
        bad: Option<T>,
        array: &[T],
        dims: [usize; 2],
    ) -> AstResult<()> {
        let method = format!("astAddPixelMask{}", T::CODE);
        if dims[0] < 1 || dims[1] < 1 {
            return Err(AstError::new(
                ErrorCode::Invar,
                format!(
                    "{method}(Moc): Invalid values ({},{}) supplied for parameter 'dims'.",
                    dims[0], dims[1]
                ),
            ));
        }
        if array.len() < dims[0] * dims[1] {
            return Err(AstError::new(
                ErrorCode::Invar,
                format!(
                    "{method}(Moc): The array has {} values, fewer than the {} its dimensions \
                     need.",
                    array.len(),
                    dims[0] * dims[1]
                ),
            ));
        }
        let mut picked = find_sky_axes_frameset(wcs, &method)?;

        let mut maxorder = if self.test_max_order() {
            self.max_order()
        } else {
            let pixarea = get_pixel_area(&picked, dims)?;
            let res = (0.25 * pixarea).sqrt();
            let mut order = res_to_order(res);
            let res = order_to_res(order);
            if res * res > 0.25 * pixarea {
                order += 1;
            }
            self.store_max_order(order)?;
            order
        };
        let mut minorder = self.min_order();
        let mut oversample = 0;
        if minorder >= maxorder {
            oversample = minorder - maxorder;
            maxorder = minorder;
            minorder = maxorder - 1;
        }
        if minorder < 0 {
            return Err(AstError::new(
                ErrorCode::Invar,
                format!(
                    "{method}(Moc): Invalid value ({minorder}) supplied for parameter 'MinOrder'."
                ),
            ));
        }
        picked.invert();

        // The grid bounds of the selected pixels (GetSelectionBounds<Oper><X>).
        let passes = |v: T| pixel_passes(v, value, oper);
        let (mut lx, mut hx, mut ly, mut hy) = (i64::MAX, -i64::MAX, i64::MAX, -i64::MAX);
        for iy in 1..=dims[1] {
            for ix in 1..=dims[0] {
                if passes(array[(ix - 1) + (iy - 1) * dims[0]]) {
                    let (x, y) = (ix as i64, iy as i64);
                    lx = lx.min(x);
                    hx = hx.max(x);
                    ly = ly.min(y);
                    hy = hy.max(y);
                }
            }
        }
        if !(lx <= hx && ly <= hy) {
            if cmode == CmpOperator::And {
                self.range.clear();
                self.clear_cache();
            }
            return Ok(());
        }

        let mut fs = hpx12_frameset(maxorder)?;
        fs.invert();
        let Some(fsconv) = Frame::convert_to(&picked, &fs, &[])? else {
            return Err(AstError::new(
                ErrorCode::Nodef,
                format!(
                    "{method}(Moc): The supplied pixel array cannot be mapped to HEALPix coordinates."
                ),
            ));
        };
        let array2hpx12 = fsconv.mapping(FrameRef::Base, FrameRef::Current)?;

        let mut best: Option<(usize, [i64; 4], DynMapping)> = None;
        let mut npix_min = 0_i64;
        let lbnd_in = [lx as f64 - HALF, ly as f64 - HALF];
        let ubnd_in = [hx as f64 + HALF, hy as f64 + HALF];
        for (iproj, proj) in XPH_PROJECTIONS.iter().enumerate() {
            let mut xph = XphMap::new(maxorder as u32, *proj);
            xph.invert();
            let array2proj =
                CmpMap::new(array2hpx12.clone_box(), Box::new(xph), Composition::Series)?;
            let bx = array2proj.map_box(&lbnd_in, &ubnd_in, true, 1)?;
            let by = array2proj.map_box(&lbnd_in, &ubnd_in, true, 2)?;
            let (glbnd, gubnd) = ([bx.lbnd_out, by.lbnd_out], [bx.ubnd_out, by.ubnd_out]);
            let npix = ((gubnd[0] - glbnd[0] + 1.0) * (gubnd[1] - glbnd[1] + 1.0)) as i64;
            if iproj == 0 || npix < npix_min {
                let d0 = 0.05 * (gubnd[0] - glbnd[0] + 1.0);
                let d1 = 0.05 * (gubnd[1] - glbnd[1] + 1.0);
                let b = [
                    (glbnd[0] - d0) as i64,
                    (glbnd[1] - d1) as i64,
                    (gubnd[0] + d0) as i64,
                    (gubnd[1] + d1) as i64,
                ];
                npix_min = npix;
                best = Some((iproj, b, Box::new(array2proj)));
            }
        }
        let (iproj_min, b, mut array2proj_min) = best.expect("four projections tried");
        array2proj_min.invert();
        let maps = order_maps(array2proj_min, maxorder, minorder)?;
        let b = bounds_at_minorder(&maps, maxorder, minorder, b)?;
        let (nx, ny) = (dims[0] as i64, dims[1] as i64);
        let test = |pts: &PointSet| -> AstResult<Vec<bool>> {
            Ok((0..pts.npoint())
                .map(|i| {
                    let p = pts.point(i);
                    if is_bad(p[0]) || is_bad(p[1]) {
                        return false;
                    }
                    let ix = p[0].round() as i64;
                    let iy = p[1].round() as i64;
                    if ix > 0 && ix <= nx && iy > 0 && iy <= ny {
                        let v = array[((ix - 1) + (iy - 1) * nx) as usize];
                        bad.is_none_or(|b| v != b) && passes(v)
                    } else {
                        false
                    }
                })
                .collect())
        };
        self.fill_cells(
            &maps, maxorder, minorder, oversample, iproj_min, b, &test, false, cmode, &method,
        )
    }

    /// The cells over the bounding box `b` at `minorder` that `test` finds
    /// in the region of interest, put into a CellList by moc.c `PutCell` and
    /// incorporated into the Moc by `IncorporateCells`.
    #[allow(clippy::too_many_arguments)]
    fn fill_cells(
        &mut self,
        maps: &[Option<DynMapping>],
        maxorder: i32,
        minorder: i32,
        oversample: i32,
        iproj: usize,
        b: [i64; 4],
        test: &InsideTest,
        negate: bool,
        cmode: CmpOperator,
        method: &str,
    ) -> AstResult<()> {
        let mut clist = CellList {
            maxorder,
            maps: (0..=maxorder)
                .map(|i| {
                    (i >= minorder).then(|| {
                        Box::new(XphMap::new(i as u32, XPH_PROJECTIONS[iproj])) as DynMapping
                    })
                })
                .collect(),
            ix: vec![Vec::new(); (maxorder + 1) as usize],
            iy: vec![Vec::new(); (maxorder + 1) as usize],
        };
        let mut ix = Vec::new();
        let mut iy = Vec::new();
        for j in b[1]..=b[3] {
            for i in b[0]..=b[2] {
                ix.push(i);
                iy.push(j);
            }
        }
        put_cell(maps, &ix, &iy, minorder, &mut clist, test, None, method)?;
        self.incorporate_cells(&clist, oversample, negate, cmode, method)
    }

    /// moc.c `IncorporateCells`: the cells in `clist`, at each order taken to
    /// HPX12 nested indices, sorted, gathered into ranges and taken to
    /// MaxOrder, combined with the Moc.
    fn incorporate_cells(
        &mut self,
        clist: &CellList,
        oversample: i32,
        negate: bool,
        cmode: CmpOperator,
        method: &str,
    ) -> AstResult<()> {
        let nold = self.nrange();
        for order in 0..=clist.maxorder {
            let o = order as usize;
            let len = clist.ix[o].len();
            if len == 0 {
                continue;
            }
            let pts = PointSet::from_rows(
                clist.ix[o]
                    .iter()
                    .zip(&clist.iy[o])
                    .map(|(&x, &y)| [x as f64, y as f64])
                    .collect::<Vec<_>>()
                    .iter()
                    .map(|p| p.as_slice()),
            );
            let map = clist.maps[o].as_ref().expect("map at each order in use");
            let hpx = map.transform(&pts, true)?;
            let mut nested = Vec::with_capacity(len);
            for i in 0..len {
                let p = hpx.point(i);
                if is_bad(p[0]) || is_bad(p[1]) {
                    return Err(AstError::new(
                        ErrorCode::Inter,
                        format!(
                            "{method}(Moc): Bad HPX12 grid coord element {i} order {order} \
                             (internal programming error)."
                        ),
                    ));
                }
                nested.push(xy_to_nested(
                    order,
                    p[0].round() as i32,
                    p[1].round() as i32,
                ));
            }
            nested.sort_unstable();
            let shift = 2 * (clist.maxorder - order - oversample);
            let mut k = 0;
            while k < nested.len() {
                let ilow = nested[k];
                let mut ihigh = ilow;
                k += 1;
                while k < nested.len() && nested[k] == ihigh + 1 {
                    ihigh += 1;
                    k += 1;
                }
                if shift > 0 {
                    self.range
                        .extend([ilow << shift, ((ihigh + 1) << shift) - 1]);
                } else {
                    self.range.extend([ilow >> -shift, ihigh >> -shift]);
                }
            }
        }
        // The ranges are at the Moc's own MaxOrder, below the order of the
        // finest cells by `oversample` (moc.c fixed upstream in 28473801).
        self.moc_norm(negate, cmode, nold, clist.maxorder - oversample, method)
    }
}

/// The four HPX-like projections, in the order of their C codes 0 to 3.
const XPH_PROJECTIONS: [XphProjection; 4] = [
    XphProjection::Hpx0,
    XphProjection::Hpx12,
    XphProjection::Xphn,
    XphProjection::Xphs,
];

/// Whether `v` passes the test `oper` against `value`.
fn pixel_passes<T: PixelValue>(v: T, value: T, oper: PixelOper) -> bool {
    match oper {
        PixelOper::Lt => v < value,
        PixelOper::Le => v <= value,
        PixelOper::Eq => v == value,
        PixelOper::Ne => v != value,
        PixelOper::Ge => v >= value,
        PixelOper::Gt => v > value,
    }
}

/// moc.c `GetHPX12FrameSet`: a FrameSet whose base Frame is HPX12 grid
/// coordinates at `order` and whose current Frame is ICRS.
fn hpx12_frameset(order: i32) -> AstResult<FrameSet> {
    let mut grid = crate::frames::BasicFrameBuilder::new(2).build()?;
    Frame::set_domain(&mut grid, "GRID")?;
    let mut fs = FrameSet::new(Box::new(grid));
    fs.add_frame(
        FrameRef::Base,
        Box::new(hpx12_grid_to_icrs(order)?),
        Box::new(SkyFrame::new(SkySystem::Icrs)),
    )?;
    Ok(fs)
}

/// moc.c `PutCell`: test each cell at order `order` (at its centre at
/// MaxOrder, otherwise at nine points over it), unless `inorout` says it is
/// wholly in (negative) or out (positive); keep a cell wholly in, and below
/// MaxOrder divide one partly in into its four children, each marked in or
/// out where its four corners agree, and test those in turn.
#[allow(clippy::too_many_arguments)]
fn put_cell(
    maps: &[Option<DynMapping>],
    ix: &[i64],
    iy: &[i64],
    order: i32,
    clist: &mut CellList,
    test: &InsideTest,
    inorout: Option<&[i32]>,
    method: &str,
) -> AstResult<()> {
    let ncell = ix.len();
    let npoint = if order == clist.maxorder { 1 } else { 9 };
    let unknown = |icell: usize| inorout.is_none_or(|io| io[icell] == 0);
    let mut rows: Vec<[f64; 2]> = Vec::new();
    for icell in 0..ncell {
        if !unknown(icell) {
            continue;
        }
        let (x, y) = (ix[icell] as f64, iy[icell] as f64);
        if npoint == 1 {
            rows.push([x, y]);
        } else {
            for j in -1..=1 {
                for i in -1..=1 {
                    rows.push([x + HALF * f64::from(i), y + HALF * f64::from(j)]);
                }
            }
        }
    }
    let inside = if rows.is_empty() {
        Vec::new()
    } else {
        let ps1 = PointSet::from_rows(rows.iter().map(|p| p.as_slice()));
        let map = maps[order as usize]
            .as_ref()
            .expect("map at each order in use");
        test(&map.transform(&ps1, true)?)?
    };

    let o = order as usize;
    let mut sub_ix = Vec::new();
    let mut sub_iy = Vec::new();
    let mut sub_io = Vec::new();
    let mut pin = 0;
    for icell in 0..ncell {
        let (allin, allout, flags) = if unknown(icell) {
            let flags = &inside[pin..pin + npoint];
            pin += npoint;
            (
                flags.iter().all(|&f| f),
                !flags.iter().any(|&f| f),
                Some(flags),
            )
        } else if inorout.expect("known in or out")[icell] > 0 {
            (false, true, None)
        } else {
            (true, false, None)
        };
        if allin {
            if clist.ix[o].len() >= (i32::MAX - 2) as usize {
                return Err(AstError::new(
                    ErrorCode::Bgmoc,
                    format!("{method}(Moc): The normalised MOC has too many elements."),
                ));
            }
            clist.ix[o].push(ix[icell]);
            clist.iy[o].push(iy[icell]);
        } else if !allout && order < clist.maxorder {
            let flags = flags.expect("a partly inside cell was tested");
            for (j, k0) in [(0, 0), (1, 3)] {
                for (i, dk) in [(0, 0), (1, 1)] {
                    let k = k0 + dk;
                    let corners = [flags[k], flags[k + 1], flags[k + 3], flags[k + 4]];
                    let sin = corners.iter().all(|&f| f);
                    let sout = !corners.iter().any(|&f| f);
                    sub_ix.push(2 * ix[icell] - 1 + i);
                    sub_iy.push(2 * iy[icell] - 1 + j);
                    sub_io.push(i32::from(sout) - i32::from(sin));
                }
            }
        }
    }
    if !sub_ix.is_empty() {
        put_cell(
            maps,
            &sub_ix,
            &sub_iy,
            order + 1,
            clist,
            test,
            Some(&sub_io),
            method,
        )?;
    }
    Ok(())
}

// ============================================================================
// RegionGeometry
// ============================================================================

impl RegionGeometry for Moc {
    fn region_data(&self) -> &RegionData {
        &self.data
    }

    fn region_data_mut(&mut self) -> &mut RegionData {
        &mut self.data
    }

    /// moc.c `Transform` in ICRS: the HPX12 grid cell holding each point at
    /// MaxOrder, as a nested index, searched for among the ranges.
    fn reg_base_inside(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        // An empty Moc, which may have no MaxOrder, contains no position and
        // needs no Mapping.
        if self.range.is_empty() {
            return Ok(vec![false; pts.npoint()]);
        }
        let order = self.max_order();
        let map = self.cached_mapping(order)?;
        let grid = map.transform(pts, true)?;
        Ok((0..grid.npoint())
            .map(|i| {
                let p = grid.point(i);
                let inest = xy_to_nested(order, p[0].round() as i32, p[1].round() as i32);
                self.range
                    .chunks(2)
                    .find(|r| r[1] >= inest)
                    .is_some_and(|r| r[0] <= inest)
            })
            .collect())
    }

    /// moc.c `RegBaseBox`: the box round the boundary mesh, its RA range
    /// whichever of the two taken about RA 0 and about RA 12h is narrower.
    /// An empty Moc gets the box of an empty Region (upper bounds below
    /// lower), and a Moc of the whole sky the whole sky.
    fn reg_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        if let Some((lb, ub)) = self.cache.borrow().bounds {
            return Ok((lb.to_vec(), ub.to_vec()));
        }
        if self.nrange() == 0 {
            let bounds = ([1.0, 1.0], [-1.0, -1.0]);
            self.cache.borrow_mut().bounds = Some(bounds);
            return Ok((bounds.0.to_vec(), bounds.1.to_vec()));
        }
        if self.nrange() == 1
            && self.range[0] == 0
            && self.range[1] == 12 * (1_i64 << (2 * self.max_order())) - 1
        {
            let bounds = ([0.0, -DPIBY2], [2.0 * DPI, DPIBY2]);
            self.cache.borrow_mut().bounds = Some(bounds);
            return Ok((bounds.0.to_vec(), bounds.1.to_vec()));
        }
        let mesh = self.base_mesh()?;
        let (mut mindec, mut maxdec) = (f64::MAX, -f64::MAX);
        let (mut minra0, mut maxra0) = (f64::MAX, -f64::MAX);
        let (mut minra12, mut maxra12) = (f64::MAX, -f64::MAX);
        for p in &mesh {
            let (ra, dec) = (p[0], p[1]);
            if dec < mindec {
                mindec = dec;
            }
            if dec > maxdec {
                maxdec = dec;
            }
            let mut ra12 = ra;
            while ra12 < 0.0 {
                ra12 += 2.0 * DPI;
            }
            while ra12 > 2.0 * DPI {
                ra12 -= 2.0 * DPI;
            }
            if ra12 < minra12 {
                minra12 = ra12;
            }
            if ra12 > maxra12 {
                maxra12 = ra12;
            }
            let mut ra0 = ra;
            while ra0 < -DPI {
                ra0 += 2.0 * DPI;
            }
            while ra0 > DPI {
                ra0 -= 2.0 * DPI;
            }
            if ra0 < minra0 {
                minra0 = ra0;
            }
            if ra0 > maxra0 {
                maxra0 = ra0;
            }
        }
        let (lra, ura) = if maxra0 - minra0 < maxra12 - minra12 {
            (minra0, maxra0)
        } else {
            (minra12, maxra12)
        };
        let bounds = ([lra, mindec], [ura, maxdec]);
        self.cache.borrow_mut().bounds = Some(bounds);
        Ok((bounds.0.to_vec(), bounds.1.to_vec()))
    }

    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        self.base_mesh()
    }

    /// region.c `GetBounded`: a Region on the sky is bounded.
    fn is_bounded_inner(&self) -> AstResult<bool> {
        Ok(self.data.closed_curve_bounded())
    }

    fn remap_defining_points(&mut self, _mapping: &dyn Mapping, _forward: bool) -> AstResult<()> {
        // A Moc's cells stay in ICRS, its base Frame.
        self.data.invalidate_cache();
        Ok(())
    }
}

// ============================================================================
// Region
// ============================================================================

impl Region for Moc {
    fn point_in_region(&self, point: &[f64]) -> AstResult<bool> {
        let pts = PointSet::from_rows(std::iter::once(point));
        let out = Mapping::transform(self, &pts, true)?;
        Ok(out.point(0).iter().any(|&v| !is_bad(v)))
    }

    fn negated_copy(&self) -> DynRegion {
        let mut copy = self.clone();
        copy.data.negate();
        Box::new(copy)
    }

    fn negate(&mut self) {
        self.data.negate();
    }

    fn is_negated(&self) -> bool {
        self.data.negated
    }

    fn test_negated(&self) -> bool {
        self.data.negated_set
    }

    fn set_negated_raw(&mut self, negated: bool, set: bool) {
        self.data.negated = negated;
        self.data.negated_set = set;
    }

    fn is_closed(&self) -> bool {
        self.data.closed
    }

    fn set_closed(&mut self, closed: bool) {
        self.data.closed = closed;
        self.data.closed_set = true;
    }

    fn test_closed(&self) -> bool {
        self.data.closed_set
    }

    fn set_closed_raw(&mut self, closed: bool, set: bool) {
        self.data.closed = closed;
        self.data.closed_set = set;
    }

    fn fill_factor(&self) -> f64 {
        self.data.effective_fill_factor()
    }

    fn set_fill_factor(&mut self, factor: f64) -> AstResult<()> {
        self.data.set_fill_factor(factor)
    }

    fn test_fill_factor(&self) -> bool {
        self.data.fill_factor_set
    }

    fn clear_fill_factor(&mut self) {
        self.data.clear_fill_factor();
    }

    fn mesh_size(&self) -> usize {
        self.data.effective_mesh_size()
    }

    fn set_mesh_size(&mut self, size: usize) {
        self.data.set_mesh_size(size);
    }

    fn test_mesh_size(&self) -> bool {
        self.data.mesh_size_set
    }

    fn clear_mesh_size(&mut self) {
        self.data.clear_mesh_size();
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

    fn set_region_fs(&mut self, frame: &dyn Frame) {
        self.data.set_region_fs(frame);
    }

    fn get_region_bounds(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        super::region_bounds_current(self)
    }

    fn region_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        self.reg_base_box()
    }

    fn get_region_mesh(&self) -> AstResult<PointSet> {
        let mesh = self.base_mesh()?;
        Ok(PointSet::from_rows(mesh.iter().map(|p| p.as_slice())))
    }

    fn get_region_grid(&self) -> AstResult<PointSet> {
        super::data::region_base_grid(self)
    }

    fn get_region_points(&self) -> AstResult<PointSet> {
        Ok(PointSet::zeros(0, 2))
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

    fn reg_frame_attrib<'a>(
        &mut self,
        setting: &super::FrameSetting<'a>,
    ) -> AstResult<super::FrameSetting<'a>> {
        super::data::region_frame_attrib(self, setting)
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

    fn get_uncertainty(&self) -> Option<DynRegion> {
        self.data.uncertainty.clone()
    }

    /// moc.c `GetDefUnc`: a Circle at the ICRS origin whose radius is the
    /// resolution at MaxOrder.
    fn get_def_unc(&self) -> AstResult<DynRegion> {
        if let Some(unc) = &self.cache.borrow().unc {
            return Ok(unc.clone_region());
        }
        let res = DD2R * order_to_res(self.max_order()) / 3600.0;
        let bfrm = self.data.frameset.base().snapshot();
        let unc: DynRegion = Box::new(
            CircleBuilder::from_dyn(bfrm, vec![0.0, 0.0])?
                .radius(res)
                .build()?,
        );
        self.cache.borrow_mut().unc = Some(unc.clone_region());
        Ok(unc)
    }

    fn reg_perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        self.data.perm_axes(perm)
    }

    /// moc.c `RegPins`: each point is on the boundary if a mesh point lies
    /// within the resolution at MaxOrder plus the diagonal of the
    /// uncertainty's bounding box of it, in Dec and in RA scaled by
    /// cos(Dec). The points are taken in order of Dec, then RA.
    fn reg_pins_mask(
        &self,
        pts: &PointSet,
        unc: Option<&dyn Region>,
    ) -> AstResult<(bool, Vec<bool>)> {
        if pts.ncoord() != 2 {
            return Err(AstError::new(
                ErrorCode::Inter,
                format!(
                    "astRegPins(Moc): Illegal number of axis values per point ({}) in the \
                     supplied PointSet - should be 2 (internal AST programming error).",
                    pts.ncoord()
                ),
            ));
        }
        if let Some(u) = unc
            && u.naxes() != 2
        {
            return Err(AstError::new(
                ErrorCode::Inter,
                format!(
                    "astRegPins(Moc): Illegal number of axes ({}) in the supplied uncertainty \
                     Region - should be 2 (internal AST programming error).",
                    u.naxes()
                ),
            ));
        }
        let l1 = DD2R * order_to_res(self.max_order()) / 3600.0;
        let l2 = match unc {
            Some(u) => {
                let (lbnd, ubnd) = u.get_region_bounds()?;
                u.distance(&lbnd, &ubnd)?
            }
            None => 0.0,
        };
        let drad = l1 + l2;
        let mesh = self.base_mesh()?;
        let len_mesh = mesh.len();
        let len_pins = pts.npoint();

        let mut index: Vec<usize> = (0..len_pins).collect();
        let decra = |i: usize| (pts.point(i)[1], pts.point(i)[0]);
        index.sort_by(|&a, &b| {
            let (da, ra) = decra(a);
            let (db, rb) = decra(b);
            da.partial_cmp(&db)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(ra.partial_cmp(&rb).unwrap_or(std::cmp::Ordering::Equal))
        });

        let mut mask = vec![false; len_pins];
        let mut result = true;
        let mut imesh = 0;
        for &ipin in &index {
            let (dec_pin, ra_pin) = decra(ipin);
            let cosdec = dec_pin.cos();
            let (ra_low, ra_high) = if cosdec > 0.0 {
                let dra = drad / dec_pin.cos();
                (ra_pin - dra, ra_pin + dra)
            } else {
                (-f64::MAX, f64::MAX)
            };
            let dec_low = dec_pin - drad;
            let dec_high = dec_pin + drad;
            while imesh < len_mesh && mesh[imesh][1] < dec_low {
                imesh += 1;
            }
            if imesh == len_mesh {
                result = false;
                break;
            }
            let mut on = false;
            for p in &mesh[imesh..] {
                if p[1] > dec_high {
                    break;
                }
                let mut dra = p[0] - ra_pin;
                while dra >= DPI {
                    dra -= 2.0 * DPI;
                }
                while dra < -DPI {
                    dra += 2.0 * DPI;
                }
                dra += ra_pin;
                if dra >= ra_low && dra <= ra_high {
                    on = true;
                    break;
                }
            }
            if on {
                mask[ipin] = true;
            } else {
                result = false;
            }
        }
        Ok((result, mask))
    }

    fn clone_region(&self) -> DynRegion {
        Box::new(self.clone())
    }
}

// ============================================================================
// Frame
// ============================================================================

impl Frame for Moc {
    region_frame_attrs!();

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
        match other.as_any().downcast_ref::<Moc>() {
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

impl Mapping for Moc {
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

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        super::data::region_transform(self, pts)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        crate::mappings::validate_transform(self, pts, forward, Some(out))?;
        let result = self.transform(pts, forward)?;
        let ncoord = result.ncoord();
        for i in 0..result.npoint() {
            out.point_mut(i)[..ncoord].copy_from_slice(result.point(i));
        }
        Ok(())
    }

    /// A Moc inherits region.c `Simplify`, which simplifies the FrameSet and
    /// the uncertainty.
    fn simplified(&self, _gate: ViaSimplify) -> AstResult<(DynMapping, bool)> {
        match self.parent_region_simplify()? {
            Some(new) => Ok((new.clone_box(), true)),
            None => Ok((self.clone_box(), false)),
        }
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    /// moc.c `Equal`: region.c `Equal`, then the same MaxOrder and ranges.
    fn equal(&self, other: &dyn Mapping) -> bool {
        super::data::region_equal(self, other)
            .is_some_and(|other| self.max_order() == other.max_order() && self.range == other.range)
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}
