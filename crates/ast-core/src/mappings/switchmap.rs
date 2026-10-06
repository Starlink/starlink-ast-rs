//! SwitchMap — alternate regionalised Mapping.

use std::fmt;

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::helpers::is_bad;

use super::{DynMapping, Mapping, UnitMap};
use crate::mappings::SimplifiedRecord;

/// Set a Mapping's Invert flag to `want` (invert it iff its current state
/// differs). Mirrors C `astSetInvert`.
fn set_invert(m: &mut dyn Mapping, want: bool) {
    if m.is_inverted() != want {
        m.invert();
    }
}

/// Simplify an optional selector child Mapping and re-derive its recorded
/// invert flag so the effective application direction is preserved.
///
/// Returns `(simplified, new_recorded_inv)`. Sets `changed` to true if
/// either the simplified child differs from the original or the recorded
/// invert flag had to change.
fn simplify_child_with_invert(
    child: Option<&DynMapping>,
    recorded_inv: bool,
    changed: &mut bool,
) -> AstResult<(Option<DynMapping>, bool)> {
    match child {
        None => Ok((None, recorded_inv)),
        Some(orig) => {
            let mut simp = orig.simplify()?;
            // C reaches each encapsulated Mapping through `astSimplify`
            // (switchmap.c:1079-1090), which records the result it returns
            // whether or not it reduced anything (mapping.c:24763). `simplify`
            // records it too, except for a restricted CmpMap, which this
            // covers.
            simp.set_has_been_simplified(true);
            let effective = recorded_inv != orig.is_inverted();
            let new_inv = effective != simp.is_inverted();
            if !simp.equal(orig.as_ref()) || simp.is_inverted() != orig.is_inverted() {
                *changed = true;
            }
            Ok((Some(simp), new_inv))
        }
    }
}

/// A Mapping that dispatches positions to one of several route Mappings
/// based on a selector value.
///
/// A SwitchMap encapsulates:
/// - A forward selector Mapping (used for the forward transform)
/// - An inverse selector Mapping (used for the inverse transform)
/// - A set of route Mappings, all sharing the same nin and nout
///
/// The selector Mapping produces a single output value. This value is
/// rounded to the nearest integer and used as a 1-based index into the
/// route Mappings array. The selected route Mapping is then used to
/// transform the input position.
pub struct SwitchMap {
    fsmap: Option<DynMapping>,
    ismap: Option<DynMapping>,
    route_maps: Vec<DynMapping>,
    fsinv: bool,
    isinv: bool,
    route_inv: Vec<bool>,
    inverted: bool,
    simplified: SimplifiedRecord,
    ident: Option<String>,
    intrinsic_nin: usize,
    intrinsic_nout: usize,
}

impl SwitchMap {
    /// Create a new SwitchMap.
    ///
    /// - `fsmap`: forward selector Mapping (required for forward transform)
    /// - `ismap`: inverse selector Mapping (required for inverse transform)
    /// - `route_maps`: array of route Mappings (at least one required)
    ///
    /// At least one of `fsmap` or `ismap` must be `Some`.
    /// All route Mappings must share the same `nin()` and `nout()`.
    pub fn new(
        fsmap: Option<DynMapping>,
        ismap: Option<DynMapping>,
        route_maps: Vec<DynMapping>,
    ) -> AstResult<Self> {
        if route_maps.is_empty() {
            return Err(AstError::new(
                ErrorCode::Bdpar,
                "SwitchMap: at least one route Mapping is required",
            ));
        }
        if fsmap.is_none() && ismap.is_none() {
            return Err(AstError::new(
                ErrorCode::Intrd,
                "SwitchMap: at least one selector Mapping must be supplied",
            ));
        }

        let nin = route_maps[0].nin();
        let nout = route_maps[0].nout();

        for (i, rm) in route_maps.iter().enumerate().skip(1) {
            if rm.nin() != nin {
                return Err(AstError::new(
                    ErrorCode::Badni,
                    format!(
                        "SwitchMap: route Mapping {} has {} inputs but route 1 has {}",
                        i + 1,
                        rm.nin(),
                        nin
                    ),
                ));
            }
            if rm.nout() != nout {
                return Err(AstError::new(
                    ErrorCode::Badno,
                    format!(
                        "SwitchMap: route Mapping {} has {} outputs but route 1 has {}",
                        i + 1,
                        rm.nout(),
                        nout
                    ),
                ));
            }
        }

        if let Some(ref fs) = fsmap {
            if fs.nin() != nin {
                return Err(AstError::new(
                    ErrorCode::Badni,
                    format!(
                        "SwitchMap: forward selector has {} inputs but routes have {}",
                        fs.nin(),
                        nin
                    ),
                ));
            }
            if fs.nout() != 1 {
                return Err(AstError::new(
                    ErrorCode::Badno,
                    format!(
                        "SwitchMap: forward selector has {} outputs but should have 1",
                        fs.nout()
                    ),
                ));
            }
        }

        if let Some(ref is) = ismap {
            if is.nout() != nout {
                return Err(AstError::new(
                    ErrorCode::Badno,
                    format!(
                        "SwitchMap: inverse selector has {} outputs but routes have {}",
                        is.nout(),
                        nout
                    ),
                ));
            }
            if is.nin() != 1 {
                return Err(AstError::new(
                    ErrorCode::Badni,
                    format!(
                        "SwitchMap: inverse selector has {} inputs but should have 1",
                        is.nin()
                    ),
                ));
            }
        }

        let fsinv = fsmap.as_ref().map(|m| m.is_inverted()).unwrap_or(false);
        let isinv = ismap.as_ref().map(|m| m.is_inverted()).unwrap_or(false);
        let route_inv: Vec<bool> = route_maps.iter().map(|m| m.is_inverted()).collect();

        Ok(Self {
            fsmap,
            ismap,
            route_maps,
            fsinv,
            isinv,
            route_inv,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            intrinsic_nin: nin,
            intrinsic_nout: nout,
        })
    }

    /// Construct from pre-recorded invert flags (used by deserialization).
    #[allow(dead_code)]
    pub(crate) fn from_parts(
        fsmap: Option<DynMapping>,
        ismap: Option<DynMapping>,
        route_maps: Vec<DynMapping>,
        fsinv: bool,
        isinv: bool,
        route_inv: Vec<bool>,
    ) -> AstResult<Self> {
        if route_maps.is_empty() {
            return Err(AstError::new(
                ErrorCode::Bdpar,
                "SwitchMap: at least one route Mapping is required",
            ));
        }
        let nin = route_maps[0].nin();
        let nout = route_maps[0].nout();
        Ok(Self {
            fsmap,
            ismap,
            route_maps,
            fsinv,
            isinv,
            route_inv,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            intrinsic_nin: nin,
            intrinsic_nout: nout,
        })
    }

    /// Normalise an inverted SwitchMap into an equivalent non-inverted
    /// SwitchMap, by swapping the forward/inverse selectors and inverting
    /// each route Mapping.
    ///
    /// Returns `None` if `self` is not inverted (no normalisation needed).
    /// This is the inversion-fold path of C `switchmap.c::MapMerge`
    /// (`switchmap.c:1057-1076`), reached from [`SwitchMap::map_merge`] when the
    /// SwitchMap is applied in the inverse direction.
    pub(crate) fn normalize_inverted(&self) -> AstResult<Option<Self>> {
        if !self.inverted {
            return Ok(None);
        }
        let mut new_routes: Vec<DynMapping> = Vec::with_capacity(self.route_maps.len());
        for rm in &self.route_maps {
            let mut r = rm.clone();
            r.invert();
            new_routes.push(r);
        }
        let new_fsmap = self.ismap.clone().map(|mut m| {
            m.invert();
            m
        });
        let new_ismap = self.fsmap.clone().map(|mut m| {
            m.invert();
            m
        });
        let normalized = SwitchMap::new(new_fsmap, new_ismap, new_routes)?;
        Ok(Some(normalized))
    }

    /// The forward selector Mapping (if any).
    pub fn forward_selector(&self) -> Option<&dyn Mapping> {
        self.fsmap.as_deref()
    }

    /// The inverse selector Mapping (if any).
    pub fn inverse_selector(&self) -> Option<&dyn Mapping> {
        self.ismap.as_deref()
    }

    /// The route Mappings.
    pub fn route_maps(&self) -> &[DynMapping] {
        &self.route_maps
    }

    /// Number of route Mappings.
    pub fn nroute(&self) -> usize {
        self.route_maps.len()
    }

    /// Recorded forward selector invert flag.
    #[allow(dead_code)]
    pub(crate) fn fsinv(&self) -> bool {
        self.fsinv
    }

    /// Recorded inverse selector invert flag.
    #[allow(dead_code)]
    pub(crate) fn isinv(&self) -> bool {
        self.isinv
    }

    /// Recorded route invert flags.
    #[allow(dead_code)]
    pub(crate) fn route_inv(&self) -> &[bool] {
        &self.route_inv
    }

    /// Return the selector Mapping to use for the given transform direction,
    /// as a clone whose Invert flag is already set so that calling
    /// `.transform(pts, forward)` (with the *plain* `forward` flag) applies
    /// the correct selector. Faithful port of C `switchmap.c::GetSelector`
    /// (521): pick ismap/fsmap by `(swinv XOR fwd)`, restore its recorded
    /// invert flag, then invert again if the SwitchMap itself is inverted.
    fn resolve_selector(&self, forward: bool) -> Option<DynMapping> {
        let swinv = self.inverted;
        let (base, recorded_inv) = if (!swinv && !forward) || (swinv && forward) {
            (self.ismap.as_ref()?, self.isinv)
        } else {
            (self.fsmap.as_ref()?, self.fsinv)
        };
        let mut m = base.clone();
        set_invert(m.as_mut(), recorded_inv);
        if swinv {
            m.invert();
        }
        Some(m)
    }

    /// Return the route Mapping for `route_idx` as a clone whose Invert flag
    /// is set so that `.transform(pts, forward)` applies the correct
    /// direction. Faithful port of C `switchmap.c::GetRoute` (429): restore
    /// the recorded invert flag, then invert again if the SwitchMap is
    /// inverted.
    fn resolve_route(&self, route_idx: usize) -> DynMapping {
        let mut m = self.route_maps[route_idx].clone();
        set_invert(m.as_mut(), self.route_inv[route_idx]);
        if self.inverted {
            m.invert();
        }
        m
    }

    /// Simplify each encapsulated Mapping and rebuild the SwitchMap around the
    /// results (`switchmap.c:1077-1100`). The returned flag is C's `simp`: it
    /// reports whether any child's simplification returned a different Mapping,
    /// and is false alongside a clone of `self` when none did.
    ///
    /// The flag is what decides a rebuild, rather than a comparison of the two
    /// SwitchMaps, because C compares child pointers (`sfsmap != map->fsmap`)
    /// and a rebuilt SwitchMap can be behaviourally equal to the original: an
    /// inverted child replaced by its forward equal transforms identically.
    ///
    /// When a child's simplification changes its `is_inverted()` flag (e.g.
    /// `ZoomMap(2, inverted)` → `ZoomMap(0.5, forward)`), the recorded
    /// `fsinv` / `isinv` / `route_inv` is re-derived so that the effective
    /// application direction (`recorded_inv != is_inverted`) is preserved.
    /// Otherwise the SwitchMap would silently flip the child's direction.
    fn reduced_children(&self) -> AstResult<(SwitchMap, bool)> {
        let mut changed = false;

        let (simp_fs, new_fsinv) =
            simplify_child_with_invert(self.fsmap.as_ref(), self.fsinv, &mut changed)?;

        let (simp_is, new_isinv) =
            simplify_child_with_invert(self.ismap.as_ref(), self.isinv, &mut changed)?;

        let mut simp_routes: Vec<DynMapping> = Vec::with_capacity(self.route_maps.len());
        let mut simp_route_inv: Vec<bool> = Vec::with_capacity(self.route_maps.len());
        for (rm, &recorded_inv) in self.route_maps.iter().zip(self.route_inv.iter()) {
            let mut s = rm.simplify()?;
            s.set_has_been_simplified(true);
            let effective_inv = recorded_inv != rm.is_inverted();
            let new_inv = effective_inv != s.is_inverted();
            if !s.equal(rm.as_ref()) || s.is_inverted() != rm.is_inverted() {
                changed = true;
            }
            simp_routes.push(s);
            simp_route_inv.push(new_inv);
        }

        if !changed {
            return Ok((self.clone(), false));
        }

        let new_fs = simp_fs.or_else(|| self.fsmap.clone());
        let new_is = simp_is.or_else(|| self.ismap.clone());
        let mut new_sw = SwitchMap::from_parts(
            new_fs,
            new_is,
            simp_routes,
            new_fsinv,
            new_isinv,
            simp_route_inv,
        )?;
        if self.inverted {
            new_sw.invert();
        }
        Ok((new_sw, true))
    }
}

impl fmt::Debug for SwitchMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SwitchMap")
            .field("nin", &self.nin())
            .field("nout", &self.nout())
            .field("nroute", &self.route_maps.len())
            .field("inverted", &self.inverted)
            .finish()
    }
}

impl Clone for SwitchMap {
    fn clone(&self) -> Self {
        SwitchMap {
            fsmap: self.fsmap.clone(),
            ismap: self.ismap.clone(),
            route_maps: self.route_maps.clone(),
            fsinv: self.fsinv,
            isinv: self.isinv,
            route_inv: self.route_inv.clone(),
            inverted: self.inverted,
            simplified: self.simplified,
            ident: self.ident.clone(),
            intrinsic_nin: self.intrinsic_nin,
            intrinsic_nout: self.intrinsic_nout,
        }
    }
}

impl Mapping for SwitchMap {
    fn nin(&self) -> usize {
        if self.inverted {
            self.intrinsic_nout
        } else {
            self.intrinsic_nin
        }
    }

    fn nout(&self) -> usize {
        if self.inverted {
            self.intrinsic_nin
        } else {
            self.intrinsic_nout
        }
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn has_forward(&self) -> bool {
        self.fsmap.is_some()
    }

    fn has_inverse(&self) -> bool {
        self.ismap.is_some()
    }

    fn dump_tran_flags(&self) -> (bool, bool) {
        // C passes the computed selector availability to astInitMapping
        // (switchmap.c).
        (self.fsmap.is_some(), self.ismap.is_some())
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

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let effective_fwd = forward != self.inverted;

        let ncin = if effective_fwd {
            self.intrinsic_nin
        } else {
            self.intrinsic_nout
        };
        let ncout = if effective_fwd {
            self.intrinsic_nout
        } else {
            self.intrinsic_nin
        };
        let npoint = pts.npoint();
        let nroute = self.route_maps.len();

        // Resolve the selector into a clone whose Invert flag is already set
        // so it is applied with the plain `forward` flag (C GetSelector +
        // astTransform(selmap, in, forward, ...)).
        let sel = self.resolve_selector(forward).ok_or_else(|| {
            AstError::new(
                ErrorCode::Intrd,
                format!(
                    "SwitchMap: no {} selector Mapping available",
                    if effective_fwd { "forward" } else { "inverse" }
                ),
            )
        })?;

        let sel_out = sel.transform(pts, forward)?;
        let sel_vals: Vec<f64> = (0..npoint).map(|i| sel_out.point(i)[0]).collect();

        let mut pop: Vec<usize> = vec![0; nroute];
        for &sv in &sel_vals {
            if !is_bad(sv) {
                let idx = sv.round() as i64 - 1;
                if idx >= 0 && (idx as usize) < nroute {
                    pop[idx as usize] += 1;
                }
            }
        }

        let total_assigned: usize = pop.iter().sum();

        let mut out = PointSet::zeros(npoint, ncout);

        if total_assigned < npoint {
            for i in 0..npoint {
                for j in 0..ncout {
                    out.point_mut(i)[j] = BAD_VALUE;
                }
            }
        }

        for (iroute, &count) in pop.iter().enumerate().take(nroute) {
            if count == 0 {
                continue;
            }

            let route = self.resolve_route(iroute);

            let mut batch_in = PointSet::zeros(count, ncin);
            let mut point_indices: Vec<usize> = Vec::with_capacity(count);
            let mut k = 0;
            for (ipoint, &sv) in sel_vals.iter().enumerate() {
                if !is_bad(sv) {
                    let idx = sv.round() as i64 - 1;
                    if idx == iroute as i64 {
                        for j in 0..ncin {
                            batch_in.point_mut(k)[j] = pts.point(ipoint)[j];
                        }
                        point_indices.push(ipoint);
                        k += 1;
                    }
                }
            }

            let batch_out = route.transform(&batch_in, forward)?;

            for (k, &ipoint) in point_indices.iter().enumerate() {
                for j in 0..ncout {
                    out.point_mut(ipoint)[j] = batch_out.point(k)[j];
                }
            }
        }

        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let result = self.transform(pts, forward)?;
        out.view_mut().assign(&result.view());
        Ok(())
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        let sel = match self.resolve_selector(true) {
            Some(s) => s,
            None => return crate::mappings::rate::numeric_rate(self, at, ax1, ax2),
        };

        let pts = PointSet::from_rows([at]);
        let sel_out = sel.transform(&pts, true)?;
        let sv = sel_out.point(0)[0];

        if is_bad(sv) {
            return Ok(BAD_VALUE);
        }

        let idx = sv.round() as i64 - 1;
        if idx < 0 || idx as usize >= self.route_maps.len() {
            return Ok(BAD_VALUE);
        }

        // The resolved route is already oriented for a forward application;
        // its rate is the SwitchMap's forward rate. If the route ends up
        // applied in its inverse direction, fall back to a numeric estimate.
        let route = self.resolve_route(idx as usize);
        if !route.is_inverted() {
            route.rate(at, ax1, ax2)
        } else {
            crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
        }
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other_sw) = other.as_any().downcast_ref::<SwitchMap>() else {
            return false;
        };
        if self.route_maps.len() != other_sw.route_maps.len() {
            return false;
        }

        // Compare the two SwitchMaps *semantically*, resolving each one's own
        // Invert flag into its selectors and routes (C `switchmap.c::Equal`
        // via GetSelector/GetRoute, 229). This makes an inverted SwitchMap
        // compare equal to the equivalent non-inverted normal form.
        let sel_eq = |a: Option<DynMapping>, b: Option<DynMapping>| match (a, b) {
            (Some(a), Some(b)) => a.equal(b.as_ref()),
            (None, None) => true,
            _ => false,
        };

        // Forward selector.
        if !sel_eq(self.resolve_selector(true), other_sw.resolve_selector(true)) {
            return false;
        }
        // Inverse selector.
        if !sel_eq(
            self.resolve_selector(false),
            other_sw.resolve_selector(false),
        ) {
            return false;
        }
        // Route Mappings (resolved for a forward application).
        for i in 0..self.route_maps.len() {
            let a = self.resolve_route(i);
            let b = other_sw.resolve_route(i);
            if !a.equal(b.as_ref()) {
                return false;
            }
        }
        true
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        // C cancels the SwitchMap against a neighbour first
        // (switchmap.c:992-1051) and attempts the self-reduction only when that
        // made no change (:1052-1056). The order matters: an effectively
        // inverted SwitchMap normalised first would no longer carry the opposite
        // invert flag its partner cancels against.
        if let Some(modified) = super::simplify::default_map_merge(ctx, where_, series) {
            return Some(modified);
        }

        // The self-reduction is an either/or keyed on the *list's* invert flag
        // rather than the SwitchMap's own (:1057-1100): an effectively inverted
        // SwitchMap is replaced by the equal forward form, applied forward, and
        // nothing else happens on this pass; only a forward one has its
        // encapsulated Mappings simplified. Reporting the change brings the
        // nominate loop back to do the children over the new object. Tracing C
        // on switchmap_invert_normalize shows exactly that: the same SwitchMap
        // nominated at `inv=1`, then again at `inv=0`, then a third time once
        // the children had been reduced.
        if ctx.entry(where_).1 != self.is_inverted() {
            let mut inverted = self.clone();
            if !inverted.is_inverted() {
                inverted.invert();
            }
            let normalized = inverted.normalize_inverted().ok()??;
            ctx.set_pair_single(where_, (Box::new(normalized), false, false));
            return Some(where_);
        }

        let (reduced, changed) = self.reduced_children().ok()?;
        if changed {
            let (eff, orig) = {
                let e = ctx.entry(where_);
                (e.1, e.2)
            };
            ctx.set_pair_single(where_, (Box::new(reduced), eff, orig));
            return Some(where_);
        }
        None
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

        let other_sw = other.as_any().downcast_ref::<SwitchMap>()?;

        if self_inv == other_inv {
            return None;
        }

        if self.route_maps.len() != other_sw.route_maps.len() {
            return None;
        }
        let (self_fs, self_is) = if self_inv {
            (&self.ismap, &self.fsmap)
        } else {
            (&self.fsmap, &self.ismap)
        };
        let (other_fs, other_is) = if other_inv {
            (&other_sw.ismap, &other_sw.fsmap)
        } else {
            (&other_sw.fsmap, &other_sw.ismap)
        };
        match (self_fs, other_fs) {
            (Some(a), Some(b)) => {
                if !a.equal(b.as_ref()) {
                    return None;
                }
            }
            (None, None) => {}
            _ => return None,
        }
        match (self_is, other_is) {
            (Some(a), Some(b)) => {
                if !a.equal(b.as_ref()) {
                    return None;
                }
            }
            (None, None) => {}
            _ => return None,
        }
        for (a, b) in self.route_maps.iter().zip(other_sw.route_maps.iter()) {
            if !a.equal(b.as_ref()) {
                return None;
            }
        }

        Some(Box::new(UnitMap::new(self.intrinsic_nin)))
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

impl crate::serial::Serializable for SwitchMap {
    fn class_name(&self) -> &'static str {
        "SwitchMap"
    }

    fn to_node(&self) -> AstResult<crate::serial::Node> {
        use crate::serial::{MappingToNode, Node, write_mapping_base};

        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Alternate regionalised Mapping".to_string());
        write_mapping_base(&mut node, self);

        if let Some(fs) = self.forward_selector() {
            let fs_node = MappingToNode::to_node(fs)?;
            node.push_object(
                "FSMap",
                fs_node,
                Some("Forward selector Mapping".to_string()),
            );
            if self.fsinv {
                node.push_int(
                    "FSInv",
                    1,
                    Some("Fwd selector used in inverse direction".to_string()),
                );
            }
        }

        if let Some(is) = self.inverse_selector() {
            let is_node = MappingToNode::to_node(is)?;
            node.push_object(
                "ISMap",
                is_node,
                Some("Inverse selector Mapping".to_string()),
            );
            if self.isinv {
                node.push_int(
                    "ISInv",
                    1,
                    Some("Inv selector used in inverse direction".to_string()),
                );
            }
        }

        for (i, rm) in self.route_maps.iter().enumerate() {
            let key = format!("RMap{}", i + 1);
            let rm_node = MappingToNode::to_node(rm.as_ref())?;
            node.push_object(&key, rm_node, Some("Route Mapping".to_string()));

            if self.route_inv[i] {
                let inv_key = format!("RInv{}", i + 1);
                node.push_int(
                    &inv_key,
                    1,
                    Some("Route Mapping used in inverse direction".to_string()),
                );
            }
        }

        Ok(node)
    }

    fn from_node(node: &crate::serial::Node) -> AstResult<Self> {
        use crate::serial::{mapping_from_node, read_mapping_base};

        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("SwitchMap");
        let base = read_mapping_base(whole);

        let fsmap = node
            .get_object("FSMap")
            .map(mapping_from_node)
            .transpose()?;
        let fsinv = node.get_int("FSInv", 0) != 0;

        let ismap = node
            .get_object("ISMap")
            .map(mapping_from_node)
            .transpose()?;
        let isinv = node.get_int("ISInv", 0) != 0;

        let mut route_maps: Vec<DynMapping> = Vec::new();
        let mut route_inv: Vec<bool> = Vec::new();
        let mut i = 1;
        loop {
            let key = format!("RMap{}", i);
            match node.get_object(&key) {
                Some(rm_node) => {
                    route_maps.push(mapping_from_node(rm_node)?);
                    let inv_key = format!("RInv{}", i);
                    route_inv.push(node.get_int(&inv_key, 0) != 0);
                }
                None => break,
            }
            i += 1;
        }

        if route_maps.is_empty() {
            return Err(AstError::new(
                ErrorCode::Lderr,
                "SwitchMap: no route Mappings found in serialized data",
            ));
        }

        let mut sw = SwitchMap::from_parts(fsmap, ismap, route_maps, fsinv, isinv, route_inv)?;
        if base.inverted {
            sw.invert();
        }

        sw.set_has_been_simplified(base.has_been_simplified);
        Ok(sw)
    }
}
