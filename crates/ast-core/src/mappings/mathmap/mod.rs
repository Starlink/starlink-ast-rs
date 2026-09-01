//! MathMap: transform coordinates using mathematical expressions.
//!
//! A `MathMap` allows specifying forward and/or inverse transformation
//! functions using arithmetic operations and mathematical functions similar
//! to those available in C. The expressions are interpreted at run-time.

mod evaluator;
pub(crate) mod opcode;
mod parser;
pub(crate) mod rng;
mod token;

use std::cell::UnsafeCell;

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};

use crate::mappings::SimplifiedRecord;
use opcode::CompiledExpr;
use rng::MathMapRng;

/// A Mapping defined by user-supplied mathematical expressions.
///
/// The forward transformation converts input coordinates to output
/// using the forward expressions; the inverse uses the inverse expressions.
/// Expressions support arithmetic, trigonometric, logarithmic, and other
/// functions, plus random number generation.
pub struct MathMap {
    nin: usize,
    nout: usize,
    /// The cleaned forward expressions (lowercase, no whitespace).
    fwd_exprs: Vec<String>,
    /// The cleaned inverse expressions.
    inv_exprs: Vec<String>,
    /// Compiled forward expressions.
    fwd_compiled: Vec<CompiledExpr>,
    /// Compiled inverse expressions.
    inv_compiled: Vec<CompiledExpr>,
    /// Random number generator context.
    /// UnsafeCell because transform takes &self but needs to mutate the RNG.
    /// This is safe because MathMap is Send but not Sync.
    rng: UnsafeCell<MathMapRng>,
    /// Forward-inverse pairs simplify?
    simp_fi: Option<bool>,
    /// Inverse-forward pairs simplify?
    simp_if: Option<bool>,
    inverted: bool,
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// Whether the forward transform is defined.
    has_forward: bool,
    /// Whether the inverse transform is defined.
    has_inverse: bool,
}

// Safety: MathMap is Send but not Sync (the RNG is mutated during transform).
// This matches the design constraint: "Send, not Sync".
unsafe impl Send for MathMap {}

impl std::fmt::Debug for MathMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MathMap")
            .field("nin", &self.nin)
            .field("nout", &self.nout)
            .field("fwd_exprs", &self.fwd_exprs)
            .field("inv_exprs", &self.inv_exprs)
            .field("inverted", &self.inverted)
            .finish_non_exhaustive()
    }
}

impl Clone for MathMap {
    fn clone(&self) -> Self {
        // Safety: we have exclusive access during clone (single-threaded or Send)
        let rng = unsafe { (*self.rng.get()).clone() };
        Self {
            nin: self.nin,
            nout: self.nout,
            fwd_exprs: self.fwd_exprs.clone(),
            inv_exprs: self.inv_exprs.clone(),
            fwd_compiled: self.fwd_compiled.clone(),
            inv_compiled: self.inv_compiled.clone(),
            rng: UnsafeCell::new(rng),
            simp_fi: self.simp_fi,
            simp_if: self.simp_if,
            inverted: self.inverted,
            simplified: self.simplified,
            ident: self.ident.clone(),
            has_forward: self.has_forward,
            has_inverse: self.has_inverse,
        }
    }
}

impl MathMap {
    /// Create a new `MathMap`.
    ///
    /// # Arguments
    /// - `nin`: number of input coordinates
    /// - `nout`: number of output coordinates
    /// - `fwd`: forward transformation expressions (at least `nout` required)
    /// - `inv`: inverse transformation expressions (at least `nin` required)
    ///
    /// Each expression is of the form `"y = expr"` where `y` is an output
    /// variable name. Input variables are `x1`, `x2`, etc. (1-based).
    /// Intermediate variables may also be defined.
    pub fn new(nin: usize, nout: usize, fwd: &[&str], inv: &[&str]) -> AstResult<Self> {
        if nin == 0 {
            return Err(AstError::new(
                ErrorCode::Badni,
                "MathMap nin must be at least 1".to_string(),
            ));
        }
        if nout == 0 {
            return Err(AstError::new(
                ErrorCode::Badno,
                "MathMap nout must be at least 1".to_string(),
            ));
        }
        if fwd.len() < nout {
            return Err(AstError::new(
                ErrorCode::Badni,
                format!(
                    "MathMap needs at least {} forward expressions, got {}",
                    nout,
                    fwd.len()
                ),
            ));
        }
        if inv.len() < nin {
            return Err(AstError::new(
                ErrorCode::Badni,
                format!(
                    "MathMap needs at least {} inverse expressions, got {}",
                    nin,
                    inv.len()
                ),
            ));
        }

        // Clean expressions (lowercase, no whitespace)
        let fwd_clean: Vec<String> = fwd.iter().map(|s| clean_expr(s)).collect();
        let inv_clean: Vec<String> = inv.iter().map(|s| clean_expr(s)).collect();

        // Compile both directions (cross-referencing variable names)
        let compiled = compile_mapping(&fwd_clean, &inv_clean, nin, nout)?;
        let fwd_compiled = compiled.fwd;
        let inv_compiled = compiled.inv;
        let has_forward = compiled.has_fwd;
        let has_inverse = compiled.has_inv;

        Ok(Self {
            nin,
            nout,
            fwd_exprs: fwd_clean,
            inv_exprs: inv_clean,
            fwd_compiled,
            inv_compiled,
            rng: UnsafeCell::new(MathMapRng::with_default_seed()),
            simp_fi: None,
            simp_if: None,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            has_forward,
            has_inverse,
        })
    }

    /// Get the SimpFI attribute (forward-inverse pairs simplify).
    pub fn simp_fi(&self) -> bool {
        self.simp_fi.unwrap_or(false)
    }

    /// Set the SimpFI attribute.
    pub fn set_simp_fi(&mut self, val: bool) {
        // Changing this feeds this class's own simplification, so both
        // orientations' simplified records are discarded (C
        // `astClearIsSimple`, mathmap.c:4744).
        self.simplified.clear();
        self.simp_fi = Some(val);
    }

    /// Get the SimpIF attribute (inverse-forward pairs simplify).
    pub fn simp_if(&self) -> bool {
        self.simp_if.unwrap_or(false)
    }

    /// Set the SimpIF attribute.
    pub fn set_simp_if(&mut self, val: bool) {
        // Changing this feeds this class's own simplification, so both
        // orientations' simplified records are discarded (C
        // `astClearIsSimple`, mathmap.c:4744).
        self.simplified.clear();
        self.simp_if = Some(val);
    }

    /// Get the Seed attribute.
    pub fn seed(&self) -> i32 {
        // Safety: single-threaded access
        unsafe { (*self.rng.get()).seed }
    }

    /// Set the Seed attribute.
    pub fn set_seed(&mut self, seed: i32) {
        // Changing this feeds this class's own simplification, so both
        // orientations' simplified records are discarded (C
        // `astClearIsSimple`, mathmap.c:4744).
        self.simplified.clear();
        self.rng.get_mut().set_seed(seed);
    }

    /// Test whether the Seed attribute has been explicitly set.
    pub fn test_seed(&self) -> bool {
        // Safety: single-threaded access
        unsafe { (*self.rng.get()).seed_set }
    }

    /// Clear the Seed attribute back to its default.
    pub fn clear_seed(&mut self) {
        // Changing this feeds this class's own simplification, so both
        // orientations' simplified records are discarded (C
        // `astClearIsSimple`, mathmap.c:4744).
        self.simplified.clear();
        self.rng.get_mut().clear_seed();
    }
}

impl Mapping for MathMap {
    fn nin(&self) -> usize {
        if self.inverted { self.nout } else { self.nin }
    }

    fn nout(&self) -> usize {
        if self.inverted { self.nin } else { self.nout }
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn has_forward(&self) -> bool {
        if self.inverted {
            self.has_inverse
        } else {
            self.has_forward
        }
    }

    fn has_inverse(&self) -> bool {
        if self.inverted {
            self.has_forward
        } else {
            self.has_inverse
        }
    }

    fn dump_tran_flags(&self) -> (bool, bool) {
        // C passes the function-list availability to astInitMapping
        // (mathmap.c).
        (self.has_forward, self.has_inverse)
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let effective_forward = forward != self.inverted;
        let ncoord_out = if effective_forward {
            self.nout
        } else {
            self.nin
        };
        let mut out = PointSet::zeros(pts.npoint(), ncoord_out);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    /// NaN-as-bad fast path. MathMap evaluates user-supplied expressions
    /// via a compiled token list. Arithmetic operators propagate NaN
    /// cleanly per IEEE 754, but conditional operators (`>`, `<`,
    /// `==`, `?:`, `qif`) follow C-like ordered semantics that return
    /// false for NaN comparisons and can therefore silently take the
    /// "wrong" branch. Auditing every operator is out of scope; use the
    /// delegate-and-post-process pattern so the protocol-canonical
    /// sentinel reaches downstream stages without re-implementing the
    /// evaluator.
    fn transform_into_nan(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        self.transform_into(pts, forward, out)?;
        let mut view = out.view_mut();
        let flat = view.as_slice_mut().expect("PointSet is C-contiguous");
        for v in flat.iter_mut() {
            if *v == BAD_VALUE {
                *v = f64::NAN;
            }
        }
        Ok(())
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let effective_forward = forward != self.inverted;
        let npoint = pts.npoint();
        let ncoord_in = if effective_forward {
            self.nin
        } else {
            self.nout
        };
        let ncoord_out = if effective_forward {
            self.nout
        } else {
            self.nin
        };

        let exprs = if effective_forward {
            &self.fwd_compiled
        } else {
            &self.inv_compiled
        };

        if exprs.is_empty() {
            // Undefined transformation: fill with BAD
            for i in 0..npoint {
                let row = out.point_mut(i);
                for val in row.iter_mut().take(ncoord_out) {
                    *val = BAD_VALUE;
                }
            }
            return Ok(());
        }

        let nfun = exprs.len();

        // Collect input data as column vectors
        let input_data: Vec<Vec<f64>> = (0..ncoord_in)
            .map(|c| (0..npoint).map(|p| pts.point(p)[c]).collect())
            .collect();
        let input_refs: Vec<&[f64]> = input_data.iter().map(Vec::as_slice).collect();

        // Allocate intermediate workspace
        let n_intermediate = nfun.saturating_sub(ncoord_out);
        let mut intermediates: Vec<Vec<f64>> =
            (0..n_intermediate).map(|_| vec![0.0; npoint]).collect();

        // Output workspace
        let mut outputs: Vec<Vec<f64>> = (0..ncoord_out).map(|_| vec![0.0; npoint]).collect();

        // Safety: MathMap is Send but not Sync. We have exclusive logical
        // access to the RNG during a transform call.
        let rng_ptr = self.rng.get();

        // Evaluate each function.
        // We use raw pointers to work around borrow checker limitations:
        // we need immutable refs to previously computed slots while writing
        // to the current slot. This is safe because we only write to slot
        // `ifun` and read from slots `0..ifun`.
        for ifun in 0..nfun {
            let mut data_ptrs: Vec<&[f64]> = input_refs.clone();

            // Add previously computed intermediates (read-only)
            for slot in intermediates.iter().take(n_intermediate.min(ifun)) {
                // Safety: we only read from slots < ifun which are complete
                unsafe {
                    let ptr = slot.as_ptr();
                    data_ptrs.push(std::slice::from_raw_parts(ptr, npoint));
                }
            }

            // Add previously computed outputs (read-only)
            let prev_outputs = ifun.saturating_sub(n_intermediate);
            for slot in outputs.iter().take(prev_outputs) {
                unsafe {
                    let ptr = slot.as_ptr();
                    data_ptrs.push(std::slice::from_raw_parts(ptr, npoint));
                }
            }

            // Determine where this function's output goes
            let output_slice: &mut [f64] = if ifun < n_intermediate {
                &mut intermediates[ifun]
            } else {
                &mut outputs[ifun - n_intermediate]
            };

            // Safety: we have exclusive access to the RNG through the mapping
            unsafe {
                evaluator::evaluate(
                    &exprs[ifun],
                    &data_ptrs,
                    npoint,
                    output_slice,
                    &mut *rng_ptr,
                );
            }
        }

        // Copy outputs to the PointSet
        for (i, mut row) in out
            .view_mut()
            .rows_mut()
            .into_iter()
            .enumerate()
            .take(npoint)
        {
            for (coord, val) in row.iter_mut().enumerate().take(ncoord_out) {
                *val = outputs[coord][i];
            }
        }

        Ok(())
    }

    fn map_merge(
        &self,
        ctx: &mut crate::mappings::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        // A MathMap has no replace-with-simpler step. C's `MapMerge` merges it
        // only with the Mapping that follows it in series, only when that one is
        // also a MathMap, and only when both permit it via SimpFI / SimpIF
        // (`mathmap.c:4047-4083`).
        crate::mappings::simplify::default_map_merge(ctx, where_, series)
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<MathMap>() else {
            return false;
        };
        if self.nin != other.nin || self.nout != other.nout {
            return false;
        }
        if self.inverted != other.inverted {
            return false;
        }
        self.fwd_exprs == other.fwd_exprs && self.inv_exprs == other.inv_exprs
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
        let other = other.as_any().downcast_ref::<MathMap>()?;

        // Check SimpFI / SimpIF attributes
        let self_simplifies = if self_inv {
            self.simp_if()
        } else {
            self.simp_fi()
        };
        let other_simplifies = if other_inv {
            other.simp_fi()
        } else {
            other.simp_if()
        };

        if !self_simplifies || !other_simplifies {
            return None;
        }

        // Get effective dimensions
        let nin1 = if self_inv { self.nout } else { self.nin };
        let nout2 = if other_inv { other.nin } else { other.nout };
        if nin1 != nout2 {
            return None;
        }

        // Compare effective forward of self with effective inverse of other
        let fwd1: &[String] = if !self_inv {
            &self.fwd_exprs
        } else {
            &self.inv_exprs
        };
        let inv2: &[String] = if !other_inv {
            &other.inv_exprs
        } else {
            &other.fwd_exprs
        };
        if fwd1.len() != inv2.len() {
            return None;
        }
        if fwd1 != inv2 {
            return None;
        }

        // Compare effective inverse of self with effective forward of other
        let inv1: &[String] = if !self_inv {
            &self.inv_exprs
        } else {
            &self.fwd_exprs
        };
        let fwd2: &[String] = if !other_inv {
            &other.fwd_exprs
        } else {
            &other.inv_exprs
        };
        if inv1.len() != fwd2.len() {
            return None;
        }
        if inv1 != fwd2 {
            return None;
        }

        // They cancel — return a UnitMap
        Some(Box::new(UnitMap::new(nin1)))
    }

    fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    fn set_ident(&mut self, ident: &str) {
        self.ident = if ident.is_empty() {
            None
        } else {
            Some(ident.to_string())
        };
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
// Serialization
// ---------------------------------------------------------------------------

impl Serializable for MathMap {
    fn class_name(&self) -> &'static str {
        "MathMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new("MathMap");
        node.class_comment = Some("Transformation using mathematical functions".to_string());
        write_mapping_base(&mut node, self);

        // Nfwd (only if different from nout)
        let nfwd = self.fwd_exprs.len();
        if nfwd != self.nout {
            node.push_int(
                "Nfwd",
                nfwd as i64,
                Some("Number of forward transformation functions".to_string()),
            );
        }

        // Forward expressions
        for (i, expr) in self.fwd_exprs.iter().enumerate() {
            let key = format!("Fwd{}", i + 1);
            node.push_string(&key, expr, Some(format!("Forward function {}", i + 1)));
        }

        // Ninv (only if different from nin)
        let ninv = self.inv_exprs.len();
        if ninv != self.nin {
            node.push_int(
                "Ninv",
                ninv as i64,
                Some("Number of inverse transformation functions".to_string()),
            );
        }

        // Inverse expressions
        for (i, expr) in self.inv_exprs.iter().enumerate() {
            let key = format!("Inv{}", i + 1);
            node.push_string(&key, expr, Some(format!("Inverse function {}", i + 1)));
        }

        // SimpFI
        if let Some(v) = self.simp_fi {
            node.push_int(
                "SimpFI",
                if v { 1 } else { 0 },
                Some(
                    if v {
                        "Forward-inverse pairs may simplify"
                    } else {
                        "Forward-inverse pairs do not simplify"
                    }
                    .to_string(),
                ),
            );
        }

        // SimpIF
        if let Some(v) = self.simp_if {
            node.push_int(
                "SimpIF",
                if v { 1 } else { 0 },
                Some(
                    if v {
                        "Inverse-forward pairs may simplify"
                    } else {
                        "Inverse-forward pairs do not simplify"
                    }
                    .to_string(),
                ),
            );
        }

        // Seed
        // Safety: single-threaded access for serialisation
        let rng = unsafe { &*self.rng.get() };
        if rng.seed_set {
            node.push_int(
                "Seeded",
                1,
                Some("Explicit random number seed set".to_string()),
            );
            node.push_int(
                "Seed",
                rng.seed as i64,
                Some("Random number seed value".to_string()),
            );
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self>
    where
        Self: Sized,
    {
        let base = read_mapping_base(node);
        let nin = base.nin;
        let nout = base.nout;

        // Read Nfwd / Ninv with defaults
        let nfwd = {
            let v = node.get_int("Nfwd", 0);
            if v > 0 { v as usize } else { nout }
        };
        let ninv = {
            let v = node.get_int("Ninv", 0);
            if v > 0 { v as usize } else { nin }
        };

        // Read forward expressions
        let mut fwd_exprs = Vec::with_capacity(nfwd);
        for i in 0..nfwd {
            let key = format!("Fwd{}", i + 1);
            let expr = node.get_string(&key, "");
            fwd_exprs.push(expr.to_string());
        }

        // Read inverse expressions
        let mut inv_exprs = Vec::with_capacity(ninv);
        for i in 0..ninv {
            let key = format!("Inv{}", i + 1);
            let expr = node.get_string(&key, "");
            inv_exprs.push(expr.to_string());
        }

        // Compile both directions (cross-referencing variable names)
        let compiled = compile_mapping(&fwd_exprs, &inv_exprs, nin, nout)?;
        let fwd_compiled = compiled.fwd;
        let inv_compiled = compiled.inv;
        let has_forward = compiled.has_fwd;
        let has_inverse = compiled.has_inv;

        // Read attributes
        let simp_fi_raw = node.get_int("SimpFI", -1);
        let simp_fi = if simp_fi_raw >= 0 {
            Some(simp_fi_raw != 0)
        } else {
            None
        };
        let simp_if_raw = node.get_int("SimpIF", -1);
        let simp_if = if simp_if_raw >= 0 {
            Some(simp_if_raw != 0)
        } else {
            None
        };

        // Seed
        let seeded = node.get_int("Seeded", 0) != 0;
        let rng = if seeded {
            let seed = node.get_int("Seed", 0) as i32;
            MathMapRng::new(seed, true)
        } else {
            MathMapRng::with_default_seed()
        };

        Ok(Self {
            nin,
            nout,
            fwd_exprs,
            inv_exprs,
            fwd_compiled,
            inv_compiled,
            rng: UnsafeCell::new(rng),
            simp_fi,
            simp_if,
            inverted: base.inverted,
            simplified: SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified),
            ident: None,
            has_forward,
            has_inverse,
        })
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Clean an expression: lowercase, remove whitespace.
fn clean_expr(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Extract variable names from the LHS of a set of expressions.
///
/// For an assignment (`name = expr`) the LHS up to `=` is the variable name.
/// For a bare expression that is a single valid identifier (`in0`), the whole
/// expression names the variable with no defined transformation — this
/// mirrors C `ExtractVariables` (mathmap.c), which accepts a function string
/// such as `"in0"` purely to declare the variable's name.
fn extract_lhs_names(exprs: &[String]) -> Vec<String> {
    exprs
        .iter()
        .filter_map(|expr| match find_assignment(expr) {
            Some(eq_pos) => Some(expr[..eq_pos].to_string()),
            None if is_identifier(expr) => Some(expr.clone()),
            None => None,
        })
        .collect()
}

/// Whether `s` is a single bare identifier: a letter or `_` followed by
/// letters, digits or `_` (the expressions are already lower-cased and
/// whitespace-stripped by `clean_expr`).
fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Compile a set of mapping expressions.
///
/// `input_var_names` are the variable names available as inputs (e.g.,
/// extracted from the *other* direction's last N expressions).
///
/// Returns `(compiled_exprs, max_stack_size, has_transform)`.
fn compile_mapping_exprs_with_vars(
    exprs: &[String],
    input_var_names: &[String],
) -> AstResult<(Vec<CompiledExpr>, usize, bool)> {
    let nin = input_var_names.len();
    let mut var_names: Vec<String> = input_var_names.to_vec();
    let mut rhs_exprs: Vec<String> = Vec::new();
    let mut has_transform = false;

    for expr in exprs.iter() {
        // Split on '=' (but not '==')
        if let Some(eq_pos) = find_assignment(expr) {
            has_transform = true;
            let lhs = &expr[..eq_pos];
            let rhs = &expr[eq_pos + 1..];

            if rhs.is_empty() {
                return Err(AstError::new(
                    ErrorCode::Miopa,
                    format!("empty expression after '=' in \"{}\"", expr),
                ));
            }

            var_names.push(lhs.to_string());
            rhs_exprs.push(rhs.to_string());
        } else {
            rhs_exprs.push(String::new());
        }
    }

    if !has_transform {
        return Ok((Vec::new(), 0, false));
    }

    let mut compiled = Vec::with_capacity(exprs.len());
    let mut max_stack = 0;

    for (ifun, rhs) in rhs_exprs.iter().enumerate() {
        if rhs.is_empty() {
            return Err(AstError::new(
                ErrorCode::Miopa,
                "missing expression in MathMap function".to_string(),
            ));
        }

        let available_vars: Vec<&str> =
            var_names[..nin + ifun].iter().map(|s| s.as_str()).collect();

        let tokens = token::tokenize(rhs, &available_vars)?;
        let expr = parser::compile(&tokens)?;
        if expr.stack_size > max_stack {
            max_stack = expr.stack_size;
        }
        compiled.push(expr);
    }

    Ok((compiled, max_stack, true))
}

/// Find the position of the assignment '=' in an expression.
/// Skips over '==' which is a comparison operator.
fn find_assignment(expr: &str) -> Option<usize> {
    let bytes = expr.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'=' {
            // Check it's not '==' or part of '!=' '<=' '>='
            if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                continue; // skip '=='
            }
            if i > 0 && (bytes[i - 1] == b'!' || bytes[i - 1] == b'<' || bytes[i - 1] == b'>') {
                continue; // skip '!=', '<=', '>='
            }
            return Some(i);
        }
    }
    None
}

/// Result of compiling both directions of a MathMap.
struct CompiledMapping {
    fwd: Vec<CompiledExpr>,
    inv: Vec<CompiledExpr>,
    has_fwd: bool,
    has_inv: bool,
}

/// Compile forward and inverse expressions together (cross-referencing names).
#[allow(clippy::type_complexity)]
fn compile_mapping(
    fwd_exprs: &[String],
    inv_exprs: &[String],
    nin: usize,
    nout: usize,
) -> AstResult<CompiledMapping> {
    // For the forward transform, input variable names come from the last
    // `nin` inverse expressions' LHS. If the inverse doesn't define enough
    // names, fall back to x1, x2, ...
    let inv_lhs = extract_lhs_names(inv_exprs);
    let fwd_input_vars: Vec<String> = if inv_lhs.len() >= nin {
        inv_lhs[inv_lhs.len() - nin..].to_vec()
    } else {
        (1..=nin).map(|i| format!("x{}", i)).collect()
    };

    // For the inverse transform, input variable names come from the last
    // `nout` forward expressions' LHS.
    let fwd_lhs = extract_lhs_names(fwd_exprs);
    let inv_input_vars: Vec<String> = if fwd_lhs.len() >= nout {
        fwd_lhs[fwd_lhs.len() - nout..].to_vec()
    } else {
        (1..=nout).map(|i| format!("y{}", i)).collect()
    };

    let (fwd_compiled, _fwd_stack, has_fwd) =
        compile_mapping_exprs_with_vars(fwd_exprs, &fwd_input_vars)?;
    let (inv_compiled, _inv_stack, has_inv) =
        compile_mapping_exprs_with_vars(inv_exprs, &inv_input_vars)?;

    Ok(CompiledMapping {
        fwd: fwd_compiled,
        inv: inv_compiled,
        has_fwd,
        has_inv,
    })
}
