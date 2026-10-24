# Mapping Implementation Gaps Fix Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix all correctness, equality, simplification, and performance gaps between the Rust mapping implementations and the C reference library.

**Architecture:** Per-mapping fixes organized into 10 dependency-ordered phases. Each phase produces correct, tested code that subsequent phases rely on. The approach follows the C reference implementations closely (per project memory: port all C logic completely, follow C algorithmic approaches).

**Tech Stack:** Rust 2024, ndarray, starlink-ast crate. Tests use `cargo test -p starlink-ast`.

**Key files reference:**
- Source: `crates/starlink-ast/src/mappings/{helpers,matrix,winmap,zoom,shift,perm,slamap,timemap,specmap,sphmap,wcsmap,lutmap,polymap,chebymap,cmpmap,unit,simplify,rate}.rs`
- Tests: `crates/starlink-ast/tests/test_{matrixmap,winmap,zoommap,shiftmap,permmap,slamap,timemap,sphmap,wcsmap,lutmap,simplify_regression,equal}.rs`

---

## Task 1: Per-axis BAD handling — ZoomMap

**Files:**
- Modify: `crates/starlink-ast/src/mappings/zoom.rs:81-95`
- Test: `crates/starlink-ast/tests/test_zoommap.rs`

- [ ] **Step 1: Write test for per-axis BAD handling**

Add to `crates/starlink-ast/tests/test_zoommap.rs`:

```rust
#[test]
fn zoom_bad_per_axis() {
    use starlink_ast::constants::BAD_VALUE;
    use starlink_ast::coords::PointSet;
    use starlink_ast::mappings::{Mapping, ZoomMap};

    let zm = ZoomMap::new(3, 2.0).unwrap();
    // Only axis 1 is BAD; axes 0 and 2 should still compute.
    let pts = PointSet::from_rows([[1.0, BAD_VALUE, 3.0]]);
    let out = zm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 2.0);
    assert_eq!(out.point(0)[1], BAD_VALUE);
    assert_eq!(out.point(0)[2], 6.0);

    // Inverse too
    let pts_inv = PointSet::from_rows([[BAD_VALUE, 4.0, 6.0]]);
    let out_inv = zm.transform(&pts_inv, false).unwrap();
    assert_eq!(out_inv.point(0)[0], BAD_VALUE);
    assert_eq!(out_inv.point(0)[1], 2.0);
    assert_eq!(out_inv.point(0)[2], 3.0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p starlink-ast --test test_zoommap zoom_bad_per_axis`
Expected: FAIL — axis 0 returns BAD_VALUE when it should be 2.0.

- [ ] **Step 3: Fix ZoomMap transform_into**

In `crates/starlink-ast/src/mappings/zoom.rs`, replace lines 81-95:

```rust
    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        let scale = if fwd { self.factor } else { 1.0 / self.factor };
        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            for (o, &v) in outp.iter_mut().zip(inp.iter()) {
                *o = if is_bad(v) { BAD_VALUE } else { v * scale };
            }
        }
        Ok(())
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p starlink-ast --test test_zoommap zoom_bad_per_axis`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/zoom.rs crates/starlink-ast/tests/test_zoommap.rs
git commit -m "Fix ZoomMap per-axis BAD handling

Each axis is now checked independently — BAD on one axis no longer
blanks all outputs. Matches C zoommap.c:1616-1624."
```

---

## Task 2: Per-axis BAD handling — ShiftMap

**Files:**
- Modify: `crates/starlink-ast/src/mappings/shift.rs:73-88`
- Test: `crates/starlink-ast/tests/test_shiftmap.rs`

- [ ] **Step 1: Write test for per-axis BAD handling**

Add to `crates/starlink-ast/tests/test_shiftmap.rs`:

```rust
#[test]
fn shift_bad_per_axis() {
    use starlink_ast::constants::BAD_VALUE;
    use starlink_ast::coords::PointSet;
    use starlink_ast::mappings::{Mapping, ShiftMap};

    let sm = ShiftMap::new(&[10.0, 20.0, 30.0]);
    let pts = PointSet::from_rows([[1.0, BAD_VALUE, 3.0]]);
    let out = sm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 11.0);
    assert_eq!(out.point(0)[1], BAD_VALUE);
    assert_eq!(out.point(0)[2], 33.0);

    // Inverse
    let pts_inv = PointSet::from_rows([[BAD_VALUE, 25.0, 33.0]]);
    let out_inv = sm.transform(&pts_inv, false).unwrap();
    assert_eq!(out_inv.point(0)[0], BAD_VALUE);
    assert_eq!(out_inv.point(0)[1], 5.0);
    assert_eq!(out_inv.point(0)[2], 3.0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p starlink-ast --test test_shiftmap shift_bad_per_axis`
Expected: FAIL

- [ ] **Step 3: Fix ShiftMap transform_into**

In `crates/starlink-ast/src/mappings/shift.rs`, replace lines 73-88:

```rust
    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        let sign = if fwd { 1.0 } else { -1.0 };
        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            for (j, (o, &v)) in outp.iter_mut().zip(inp.iter()).enumerate() {
                *o = if is_bad(v) { BAD_VALUE } else { v + sign * self.shift[j] };
            }
        }
        Ok(())
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p starlink-ast --test test_shiftmap shift_bad_per_axis`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/shift.rs crates/starlink-ast/tests/test_shiftmap.rs
git commit -m "Fix ShiftMap per-axis BAD handling

Each axis checked independently. Matches C shiftmap.c:1039-1065."
```

---

## Task 3: Per-axis BAD handling — WinMap

**Files:**
- Modify: `crates/starlink-ast/src/mappings/winmap.rs:163-194`
- Test: `crates/starlink-ast/tests/test_winmap.rs`

- [ ] **Step 1: Write test for per-axis BAD handling**

Add to `crates/starlink-ast/tests/test_winmap.rs`:

```rust
#[test]
fn winmap_bad_per_axis() {
    use starlink_ast::constants::BAD_VALUE;
    use starlink_ast::coords::PointSet;
    use starlink_ast::mappings::{Mapping, WinMap};

    let wm = WinMap::new(&[1.0, 2.0, 3.0], &[2.0, 3.0, 4.0]).unwrap();
    // Axis 1 is BAD
    let pts = PointSet::from_rows([[5.0, BAD_VALUE, 7.0]]);
    let out = wm.transform(&pts, true).unwrap();
    // forward: shift + scale * input
    assert_eq!(out.point(0)[0], 1.0 + 2.0 * 5.0); // 11.0
    assert_eq!(out.point(0)[1], BAD_VALUE);
    assert_eq!(out.point(0)[2], 3.0 + 4.0 * 7.0); // 31.0

    // Inverse with axis 0 BAD
    let pts_inv = PointSet::from_rows([[BAD_VALUE, 20.0, 31.0]]);
    let out_inv = wm.transform(&pts_inv, false).unwrap();
    assert_eq!(out_inv.point(0)[0], BAD_VALUE);
    assert_eq!(out_inv.point(0)[1], (20.0 - 2.0) / 3.0); // 6.0
    assert_eq!(out_inv.point(0)[2], (31.0 - 3.0) / 4.0); // 7.0
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p starlink-ast --test test_winmap winmap_bad_per_axis`
Expected: FAIL

- [ ] **Step 3: Fix WinMap transform_into**

In `crates/starlink-ast/src/mappings/winmap.rs`, replace lines 163-194:

```rust
    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            if fwd {
                for (j, (o, &v)) in outp.iter_mut().zip(inp.iter()).enumerate() {
                    let shift = self.shifts[j];
                    let scale = self.scales[j];
                    if is_bad(v) || is_bad(shift) || is_bad(scale) {
                        *o = BAD_VALUE;
                    } else {
                        *o = shift + scale * v;
                    }
                }
            } else {
                for (j, (o, &v)) in outp.iter_mut().zip(inp.iter()).enumerate() {
                    let shift = self.shifts[j];
                    let scale = self.scales[j];
                    if is_bad(v) || is_bad(shift) || is_bad(scale) || scale == 0.0 {
                        *o = BAD_VALUE;
                    } else {
                        *o = (v - shift) / scale;
                    }
                }
            }
        }
        Ok(())
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p starlink-ast --test test_winmap winmap_bad_per_axis`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/winmap.rs crates/starlink-ast/tests/test_winmap.rs
git commit -m "Fix WinMap per-axis BAD handling

Each axis checked independently. Matches C winmap.c:2494-2547."
```

---

## Task 4: Per-axis BAD handling — MatrixMap

**Files:**
- Modify: `crates/starlink-ast/src/mappings/matrix.rs:93-109`
- Test: `crates/starlink-ast/tests/test_matrixmap.rs`

- [ ] **Step 1: Write test for per-axis BAD handling**

Add to `crates/starlink-ast/tests/test_matrixmap.rs`:

```rust
#[test]
fn matrix_bad_per_axis_zero_coeff() {
    use starlink_ast::constants::BAD_VALUE;
    use starlink_ast::coords::PointSet;
    use starlink_ast::mappings::{Mapping, MatrixMap};
    use ndarray::array;

    // 2x3 matrix where row 0 has a zero coefficient on input 1.
    // Row 0: [1, 0, 2]  => output[0] = 1*x + 0*y + 2*z (y is masked)
    // Row 1: [0, 1, 1]  => output[1] = 0*x + 1*y + 1*z
    let m = array![[1.0, 0.0, 2.0], [0.0, 1.0, 1.0]];
    let mm = MatrixMap::new(m).unwrap();

    // Input with y=BAD: output[0] should still compute (zero coeff masks BAD),
    // output[1] should be BAD (non-zero coeff on y).
    let pts = PointSet::from_rows([[3.0, BAD_VALUE, 5.0]]);
    let out = mm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 1.0 * 3.0 + 0.0 + 2.0 * 5.0); // 13.0
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

#[test]
fn matrix_bad_all_good_input() {
    use starlink_ast::coords::PointSet;
    use starlink_ast::mappings::{Mapping, MatrixMap};
    use ndarray::array;

    // Normal case: no BAD values, ensure still works.
    let m = array![[1.0, 2.0], [3.0, 4.0]];
    let mm = MatrixMap::new(m).unwrap();
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = mm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 5.0);  // 1*1 + 2*2
    assert_eq!(out.point(0)[1], 11.0); // 3*1 + 4*2
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p starlink-ast --test test_matrixmap matrix_bad_per_axis`
Expected: FAIL — output[0] is BAD_VALUE when it should be 13.0.

- [ ] **Step 3: Fix MatrixMap apply_matrix**

In `crates/starlink-ast/src/mappings/matrix.rs`, replace `apply_matrix` (lines 93-109):

```rust
    fn apply_matrix(m: &Array2<f64>, pts: &PointSet, out: &mut PointSet) {
        let nout = m.nrows();
        let nin = m.ncols();
        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            for row in 0..nout {
                let mut sum = 0.0;
                let mut bad = false;
                for col in 0..nin {
                    let coeff = m[[row, col]];
                    if coeff != 0.0 {
                        if is_bad(inp[col]) {
                            bad = true;
                            break;
                        }
                        sum += coeff * inp[col];
                    }
                }
                outp[row] = if bad { BAD_VALUE } else { sum };
            }
        }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p starlink-ast --test test_matrixmap`
Expected: PASS (both new tests and existing tests)

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/matrix.rs crates/starlink-ast/tests/test_matrixmap.rs
git commit -m "Fix MatrixMap per-axis BAD handling

Zero coefficients now mask BAD inputs: if matrix[row,col]==0 and
input[col]==BAD, the BAD is skipped. Only non-zero coefficients
propagate BAD. Matches C matrixmap.c:5173-5203."
```

---

## Task 5: Per-axis BAD handling — PermMap

**Files:**
- Modify: `crates/starlink-ast/src/mappings/perm.rs:139-157`
- Test: `crates/starlink-ast/tests/test_permmap.rs`

- [ ] **Step 1: Write test for per-axis BAD handling**

Add to `crates/starlink-ast/tests/test_permmap.rs`:

```rust
#[test]
fn perm_bad_per_axis() {
    use starlink_ast::constants::BAD_VALUE;
    use starlink_ast::coords::PointSet;
    use starlink_ast::mappings::{Mapping, PermMap};

    // outperm: [2, -1, 1] => out[0]=in[1], out[1]=const[0], out[2]=in[0]
    // inperm: [3, 1, 0]   => inv: out[0]=in[2], out[1]=in[0], out[2]=BAD
    let pm = PermMap::new(&[3, 1, 0], &[2, -1, 1], &[99.0]).unwrap();

    // Forward: input[0]=BAD. out[0]=in[1]=5.0 (not affected),
    //          out[1]=const=99.0 (never affected), out[2]=in[0]=BAD.
    let pts = PointSet::from_rows([[BAD_VALUE, 5.0, 7.0]]);
    let out = pm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 5.0);
    assert_eq!(out.point(0)[1], 99.0);
    assert_eq!(out.point(0)[2], BAD_VALUE);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p starlink-ast --test test_permmap perm_bad_per_axis`
Expected: FAIL — all outputs are BAD_VALUE.

- [ ] **Step 3: Fix PermMap apply_perm**

In `crates/starlink-ast/src/mappings/perm.rs`, replace `apply_perm` (lines 139-157):

```rust
    fn apply_perm(perm: &[i64], constants: &[f64], pts: &PointSet, out: &mut PointSet) {
        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            for (k, &p) in perm.iter().enumerate() {
                outp[k] = if p > 0 {
                    let src = (p as usize) - 1;
                    if is_bad(inp[src]) { BAD_VALUE } else { inp[src] }
                } else if p < 0 {
                    constants[(-p as usize) - 1]
                } else {
                    BAD_VALUE
                };
            }
        }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p starlink-ast --test test_permmap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/perm.rs crates/starlink-ast/tests/test_permmap.rs
git commit -m "Fix PermMap per-axis BAD handling

Only the selected source axis is checked for BAD; constants are
assigned unconditionally. Matches C permmap.c:1963-1999."
```

---

## Task 6: WcsMap parameter comparison in equal() and try_merge()

**Files:**
- Modify: `crates/starlink-ast/src/mappings/wcsmap.rs:370-402`
- Test: `crates/starlink-ast/tests/test_wcsmap.rs`

- [ ] **Step 1: Write tests for parameter comparison**

Add to `crates/starlink-ast/tests/test_wcsmap.rs`:

```rust
#[test]
fn wcsmap_equal_different_params() {
    use starlink_ast::mappings::{Mapping, WcsMap, WcsProjection};

    let mut w1 = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    let mut w2 = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    // Same projection, same axes => equal
    assert!(w1.equal(&w2));

    // Set different PV parameters
    w1.set_pv(2, 1, 0.5).unwrap();
    w2.set_pv(2, 1, 0.7).unwrap();
    // Different PV => NOT equal
    assert!(!w1.equal(&w2));
}

#[test]
fn wcsmap_no_cancel_different_params() {
    use starlink_ast::mappings::{Mapping, WcsMap, WcsProjection};

    let mut w1 = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    let mut w2 = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    w1.set_pv(2, 1, 0.5).unwrap();
    w2.set_pv(2, 1, 0.7).unwrap();
    // Should NOT cancel (different params)
    let result = w1.try_merge(&w2, true, false, true);
    assert!(result.is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p starlink-ast --test test_wcsmap wcsmap_equal_different_params wcsmap_no_cancel_different_params`
Expected: FAIL — currently ignores PV params.

- [ ] **Step 3: Add parameter comparison method to WcsMap**

Add a private helper method and update `equal()` and `try_merge()` in `crates/starlink-ast/src/mappings/wcsmap.rs`:

```rust
impl WcsMap {
    /// Compare projection parameters between two WcsMaps.
    fn params_equal(&self, other: &WcsMap) -> bool {
        let p1 = self.prj_params.borrow();
        let p2 = other.prj_params.borrow();
        // Compare PV parameters for each axis
        for axis in 0..2 {
            let np1 = p1.npv(axis);
            let np2 = p2.npv(axis);
            if np1 != np2 {
                return false;
            }
            for j in 0..np1 {
                if !ast_equal(p1.pv(axis, j), p2.pv(axis, j)) {
                    return false;
                }
            }
        }
        true
    }
}
```

Update `equal()` to add `&& self.params_equal(other)` at the end of the comparison.

Update `try_merge()` cancellation check to add `&& self.params_equal(other_wcs)`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p starlink-ast --test test_wcsmap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/wcsmap.rs crates/starlink-ast/tests/test_wcsmap.rs
git commit -m "WcsMap: compare PV parameters in equal() and try_merge()

Two WcsMaps with different projection parameters now correctly
report as not equal and do not cancel. Matches C wcsmap.c:892-912
and wcsmap.c:1515-1547."
```

---

## Task 7: LutMap flat-section inverse returns BAD

**Files:**
- Modify: `crates/starlink-ast/src/mappings/lutmap.rs:269`
- Test: `crates/starlink-ast/tests/test_lutmap.rs`

- [ ] **Step 1: Write test for flat-section inverse**

Add to `crates/starlink-ast/tests/test_lutmap.rs`:

```rust
#[test]
fn lutmap_flat_section_inverse_returns_bad() {
    use starlink_ast::constants::BAD_VALUE;
    use starlink_ast::coords::PointSet;
    use starlink_ast::mappings::{LutInterp, LutMap, Mapping};

    // LUT with a flat section: [0.0, 1.0, 1.0, 1.0, 2.0]
    // At input values corresponding to indices 1-3, output is constant 1.0.
    // Inverse at y=1.0 is undetermined => BAD.
    let lut = vec![0.0, 1.0, 1.0, 1.0, 2.0];
    let lm = LutMap::new(&lut, 0.0, 1.0, LutInterp::Linear).unwrap();

    let pts = PointSet::from_rows([[1.0]]); // y=1.0 hits the flat section
    let out = lm.transform(&pts, false).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p starlink-ast --test test_lutmap lutmap_flat_section_inverse_returns_bad`
Expected: FAIL — returns midpoint instead of BAD.

- [ ] **Step 3: Fix the flat-section case**

In `crates/starlink-ast/src/mappings/lutmap.rs`, replace the `dv == 0.0` block (around line 264):

```rust
        if dv == 0.0 {
            return BAD_VALUE;
        }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p starlink-ast --test test_lutmap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/lutmap.rs crates/starlink-ast/tests/test_lutmap.rs
git commit -m "LutMap: return BAD for flat-section inverse

When multiple LUT entries have the same value, the inverse is
undetermined. Return BAD_VALUE instead of midpoint.
Matches C lutmap.c:1669-1679."
```

---

## Task 8: CmpMap canonical equality

**Files:**
- Modify: `crates/starlink-ast/src/mappings/cmpmap.rs` (the `equal()` method)
- Test: `crates/starlink-ast/tests/test_equal.rs`

- [ ] **Step 1: Write test for associativity-independent equality**

Add to `crates/starlink-ast/tests/test_equal.rs`:

```rust
#[test]
fn cmpmap_equal_different_tree_structure() {
    use starlink_ast::mappings::{CmpMap, Composition, Mapping, ShiftMap, ZoomMap};

    // (A ; (B ; C)) vs ((A ; B) ; C) — same effective pipeline.
    let a = ShiftMap::new(&[1.0, 2.0]);
    let b = ZoomMap::new(2, 3.0).unwrap();
    let c = ShiftMap::new(&[4.0, 5.0]);

    // Tree 1: A ; (B ; C)
    let bc = CmpMap::new(
        Box::new(b.clone()),
        Box::new(c.clone()),
        Composition::Series,
    ).unwrap();
    let abc_1 = CmpMap::new(
        Box::new(a.clone()),
        Box::new(bc),
        Composition::Series,
    ).unwrap();

    // Tree 2: (A ; B) ; C
    let ab = CmpMap::new(
        Box::new(a.clone()),
        Box::new(b.clone()),
        Composition::Series,
    ).unwrap();
    let abc_2 = CmpMap::new(
        Box::new(ab),
        Box::new(c.clone()),
        Composition::Series,
    ).unwrap();

    assert!(abc_1.equal(&abc_2));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p starlink-ast --test test_equal cmpmap_equal_different_tree`
Expected: FAIL

- [ ] **Step 3: Implement canonical equality for CmpMap**

In `crates/starlink-ast/src/mappings/cmpmap.rs`, update the `equal()` method to flatten both CmpMaps and compare component-by-component:

```rust
    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other_cmp) = other.as_any().downcast_ref::<CmpMap>() else {
            return false;
        };
        if self.composition != other_cmp.composition {
            return false;
        }
        // Flatten both into component lists.
        let Ok(list_self) = flatten(
            self as &dyn Mapping,
            false,
            self.composition == Composition::Series,
        ) else {
            return false;
        };
        let Ok(list_other) = flatten(
            other_cmp as &dyn Mapping,
            false,
            self.composition == Composition::Series,
        ) else {
            return false;
        };
        if list_self.len() != list_other.len() {
            return false;
        }
        list_self.iter().zip(list_other.iter()).all(
            |((m1, inv1), (m2, inv2))| inv1 == inv2 && m1.equal(m2.as_ref()),
        )
    }
```

Add `use super::simplify::flatten;` to imports if not present.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p starlink-ast --test test_equal`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/cmpmap.rs crates/starlink-ast/tests/test_equal.rs
git commit -m "CmpMap: canonical equality via flattened component comparison

(A;(B;C)) now equals ((A;B);C). Matches C cmpmap.c:291-425."
```

---

## Task 9: MatrixMap equality with inversion and storage normalization

**Files:**
- Modify: `crates/starlink-ast/src/mappings/matrix.rs:290-315`
- Test: `crates/starlink-ast/tests/test_equal.rs`

- [ ] **Step 1: Write test for cross-inversion equality**

Add to `crates/starlink-ast/tests/test_equal.rs`:

```rust
#[test]
fn matrixmap_equal_cross_inversion() {
    use starlink_ast::mappings::{Mapping, MatrixMap};
    use ndarray::array;

    // Matrix M and its inverse M_inv with opposite invert flags should be equal.
    let m = array![[1.0, 2.0], [3.0, 4.0]];
    let mm1 = MatrixMap::new(m.clone()).unwrap();

    // Create a copy and invert it.
    let mut mm2 = MatrixMap::new(m).unwrap();
    mm2.invert();

    // They should NOT be equal (one forward, one inverted — different transforms)
    assert!(!mm1.equal(&mm2));

    // But a MatrixMap storing the inverse matrix (non-inverted)
    // should equal the original with invert flag set.
    let m_inv = array![[-2.0, 1.0], [1.5, -0.5]];
    let mm3 = MatrixMap::new(m_inv).unwrap();
    // mm2 (original with invert flag) should equal mm3 (inverse matrix, not inverted)
    assert!(mm2.equal(&mm3));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p starlink-ast --test test_equal matrixmap_equal_cross_inversion`
Expected: FAIL — current code rejects different invert flags.

- [ ] **Step 3: Fix MatrixMap equal()**

In `crates/starlink-ast/src/mappings/matrix.rs`, replace the `equal()` method:

```rust
    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<MatrixMap>() else {
            return false;
        };
        // Compare effective forward matrices (resolving invert flags).
        let self_fwd = if self.inverted {
            match &self.inverse {
                Some(inv) => inv.clone(),
                None => return false,
            }
        } else {
            self.matrix.clone()
        };
        let other_fwd = if other.inverted {
            match &other.inverse {
                Some(inv) => inv.clone(),
                None => return false,
            }
        } else {
            other.matrix.clone()
        };
        if self_fwd.shape() != other_fwd.shape() {
            return false;
        }
        self_fwd
            .iter()
            .zip(other_fwd.iter())
            .all(|(a, b)| ast_equal(*a, *b))
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p starlink-ast --test test_equal && cargo test -p starlink-ast --test test_matrixmap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/matrix.rs crates/starlink-ast/tests/test_equal.rs
git commit -m "MatrixMap: compare effective forward matrix in equal()

Resolves invert flag before comparing. A MatrixMap storing the
inverse with invert=true now equals one storing the forward matrix.
Matches C matrixmap.c:827-955."
```

---

## Task 10: PermMap equality with inversion resolution

**Files:**
- Modify: `crates/starlink-ast/src/mappings/perm.rs:415-430`
- Test: `crates/starlink-ast/tests/test_equal.rs`

- [ ] **Step 1: Write test for cross-inversion PermMap equality**

Add to `crates/starlink-ast/tests/test_equal.rs`:

```rust
#[test]
fn permmap_equal_cross_inversion() {
    use starlink_ast::mappings::{Mapping, PermMap};

    // A PermMap and its inverted version (swapped inperm/outperm) should be equal.
    let pm1 = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let mut pm2 = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    pm2.invert();
    // Inverted swap is still a swap — should be equal
    // (inperm and outperm are the same here, so swapping is identity)
    assert!(pm1.equal(&pm2));

    // Different case: asymmetric permutation
    let pm3 = PermMap::new(&[1, 2, 3], &[3, 1, 2], &[]).unwrap();
    // Its inverse has outperm=[2,3,1] (the inverse permutation).
    let pm4 = PermMap::new(&[3, 1, 2], &[2, 3, 1], &[]).unwrap();
    let mut pm3_inv = pm3.clone();
    pm3_inv.invert();
    // pm3 inverted should equal pm4 non-inverted (both do the same mapping)
    assert!(pm3_inv.equal(&pm4));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p starlink-ast --test test_equal permmap_equal_cross_inversion`
Expected: FAIL

- [ ] **Step 3: Fix PermMap equal()**

In `crates/starlink-ast/src/mappings/perm.rs`, replace the `equal()` method:

```rust
    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<PermMap>() else {
            return false;
        };
        // Resolve invert flags to get effective outperm/inperm.
        let self_outperm = if self.inverted { &self.inperm } else { &self.outperm };
        let self_inperm = if self.inverted { &self.outperm } else { &self.inperm };
        let other_outperm = if other.inverted { &other.inperm } else { &other.outperm };
        let other_inperm = if other.inverted { &other.outperm } else { &other.inperm };

        if self_outperm.len() != other_outperm.len()
            || self_inperm.len() != other_inperm.len()
        {
            return false;
        }
        if self_outperm != other_outperm || self_inperm != other_inperm {
            return false;
        }
        // Compare effective constant values by output position.
        for &p in self_outperm.iter().chain(self_inperm.iter()) {
            if p < 0 {
                let self_val = self.constants[(-p - 1) as usize];
                let other_val = other.constants[(-p - 1) as usize];
                if !ast_equal(self_val, other_val) {
                    return false;
                }
            }
        }
        true
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_equal && cargo test -p starlink-ast --test test_permmap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/perm.rs crates/starlink-ast/tests/test_equal.rs
git commit -m "PermMap: resolve invert flag in equal()

Compares effective permutations rather than raw arrays.
Matches C permmap.c:314-393."
```

---

## Task 11: WinMap cross-inversion equality

**Files:**
- Modify: `crates/starlink-ast/src/mappings/winmap.rs:201-221`
- Test: `crates/starlink-ast/tests/test_equal.rs`

- [ ] **Step 1: Write test**

Add to `crates/starlink-ast/tests/test_equal.rs`:

```rust
#[test]
fn winmap_equal_cross_inversion() {
    use starlink_ast::mappings::{Mapping, WinMap};

    // Forward: out = 2 + 3*x. Inverse: out = (x-2)/3 = -2/3 + (1/3)*x.
    let wm1 = WinMap::new(&[2.0], &[3.0]).unwrap();
    let mut wm1_inv = wm1.clone();
    wm1_inv.invert();

    // Construct the inverse explicitly.
    let wm2 = WinMap::new(&[-2.0 / 3.0], &[1.0 / 3.0]).unwrap();

    // wm1 inverted should equal wm2 non-inverted.
    assert!(wm1_inv.equal(&wm2));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p starlink-ast --test test_equal winmap_equal_cross_inversion`
Expected: FAIL

- [ ] **Step 3: Fix WinMap equal()**

In `crates/starlink-ast/src/mappings/winmap.rs`, replace the `equal()` method:

```rust
    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<WinMap>() else {
            return false;
        };
        if self.shifts.len() != other.shifts.len() {
            return false;
        }
        // Compare effective parameters (resolving inversion).
        let (s1, c1) = winmap_effective_params(&self.shifts, &self.scales, self.inverted);
        let (s2, c2) = winmap_effective_params(&other.shifts, &other.scales, other.inverted);
        s1.iter().zip(&s2).all(|(a, b)| ast_equal(*a, *b))
            && c1.iter().zip(&c2).all(|(a, b)| ast_equal(*a, *b))
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_equal && cargo test -p starlink-ast --test test_winmap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/winmap.rs crates/starlink-ast/tests/test_equal.rs
git commit -m "WinMap: compare effective terms in equal()

Resolves inversion before comparison. Matches C winmap.c:576-599."
```

---

## Task 12: SlaMap step concatenation and simplification

**Files:**
- Modify: `crates/starlink-ast/src/mappings/slamap.rs:1074-1105`
- Test: `crates/starlink-ast/tests/test_slamap.rs`

- [ ] **Step 1: Write test for step concatenation**

Add to `crates/starlink-ast/tests/test_slamap.rs`:

```rust
#[test]
fn slamap_merge_concatenates_steps() {
    use starlink_ast::mappings::{Mapping, SlaCvtType, SlaMap};

    // Two SlaMaps: one with FK4->FK5, one with precession.
    let mut sla1 = SlaMap::new();
    sla1.add(SlaCvtType::Fk45z, &[1950.0]);
    let mut sla2 = SlaMap::new();
    sla2.add(SlaCvtType::Prec, &[2000.0, 2010.0]);

    // Merging in series should concatenate steps.
    let merged = sla1.try_merge(&sla2, true, false, false);
    assert!(merged.is_some());
}

#[test]
fn slamap_merge_cancels_inverse_pairs() {
    use starlink_ast::mappings::{Mapping, SlaCvtType, SlaMap, UnitMap};

    // Prec(2000,2010) followed by inverted Prec(2000,2010) should cancel.
    let mut sla1 = SlaMap::new();
    sla1.add(SlaCvtType::Prec, &[2000.0, 2010.0]);
    let mut sla2 = SlaMap::new();
    sla2.add(SlaCvtType::Prec, &[2000.0, 2010.0]);

    let merged = sla1.try_merge(&sla2, true, false, true);
    assert!(merged.is_some());
    let m = merged.unwrap();
    assert!(m.as_any().is::<UnitMap>());
}

#[test]
fn slamap_merge_combines_precession() {
    use starlink_ast::mappings::{Mapping, SlaCvtType, SlaMap};

    // Prec(2000,2010) + Prec(2010,2020) should merge to Prec(2000,2020).
    let mut sla1 = SlaMap::new();
    sla1.add(SlaCvtType::Prec, &[2000.0, 2010.0]);
    let mut sla2 = SlaMap::new();
    sla2.add(SlaCvtType::Prec, &[2010.0, 2020.0]);

    let merged = sla1.try_merge(&sla2, true, false, false);
    assert!(merged.is_some());
    // Result should be a single SlaMap with Prec(2000,2020)
    let m = merged.unwrap();
    assert!(m.as_any().is::<SlaMap>());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p starlink-ast --test test_slamap slamap_merge`
Expected: FAIL

- [ ] **Step 3: Implement SlaMap step normalization and simplification**

Port the SpecMap pattern to SlaMap. In `crates/starlink-ast/src/mappings/slamap.rs`, add helper functions and replace `try_merge`:

```rust
/// Get the inverse conversion type for a given SlaMap conversion.
fn inverse_cvt(cvt: SlaCvtType) -> SlaCvtType {
    match cvt {
        SlaCvtType::Addet => SlaCvtType::Subet,
        SlaCvtType::Subet => SlaCvtType::Addet,
        SlaCvtType::Prebn => SlaCvtType::Prebn, // self-inverse with swapped args
        SlaCvtType::Prec => SlaCvtType::Prec,   // self-inverse with swapped args
        SlaCvtType::Fk45z => SlaCvtType::Fk54z,
        SlaCvtType::Fk54z => SlaCvtType::Fk45z,
        SlaCvtType::Amp => SlaCvtType::Map,
        SlaCvtType::Map => SlaCvtType::Amp,
        SlaCvtType::Ecleq => SlaCvtType::Eqecl,
        SlaCvtType::Eqecl => SlaCvtType::Ecleq,
        SlaCvtType::Galeq => SlaCvtType::Eqgal,
        SlaCvtType::Eqgal => SlaCvtType::Galeq,
        SlaCvtType::Galsup => SlaCvtType::Supgal,
        SlaCvtType::Supgal => SlaCvtType::Galsup,
        SlaCvtType::Hpceq => SlaCvtType::Eqhpc,
        SlaCvtType::Eqhpc => SlaCvtType::Hpceq,
        SlaCvtType::Hpreq => SlaCvtType::Eqhpr,
        SlaCvtType::Eqhpr => SlaCvtType::Hpreq,
        SlaCvtType::Hfk5z => SlaCvtType::Fk5hz,
        SlaCvtType::Fk5hz => SlaCvtType::Hfk5z,
        SlaCvtType::Heeq => SlaCvtType::Eqhe,
        SlaCvtType::Eqhe => SlaCvtType::Heeq,
        SlaCvtType::J2000h => SlaCvtType::Hj2000,
        SlaCvtType::Hj2000 => SlaCvtType::J2000h,
        SlaCvtType::Dh2e => SlaCvtType::De2h,
        SlaCvtType::De2h => SlaCvtType::Dh2e,
        SlaCvtType::R2h => SlaCvtType::H2r,
        SlaCvtType::H2r => SlaCvtType::R2h,
    }
}

/// Normalize a SlaMap's step list, accounting for external inversion.
/// When inverted, reverses the list and inverts each step.
fn sla_normalized_steps(sla: &SlaMap, inv: bool) -> Vec<SlaConversion> {
    if !inv {
        sla.conversions.clone()
    } else {
        sla.conversions
            .iter()
            .rev()
            .map(|cvt| {
                let inv_type = inverse_cvt(cvt.cvt_type);
                let args = if matches!(cvt.cvt_type, SlaCvtType::Prec | SlaCvtType::Prebn) {
                    // Swap epoch arguments for precession
                    let mut a = cvt.args.clone();
                    if a.len() >= 2 {
                        a.swap(0, 1);
                    }
                    a
                } else {
                    cvt.args.clone()
                };
                SlaConversion { cvt_type: inv_type, args }
            })
            .collect()
    }
}

/// Simplify a step list: cancel inverse pairs, combine precessions.
fn sla_simplify_steps(steps: &mut Vec<SlaConversion>) -> bool {
    let mut changed = false;
    let mut i = 0;
    while i + 1 < steps.len() {
        // Check for inverse pair cancellation
        let cancel = {
            let a = &steps[i];
            let b = &steps[i + 1];
            inverse_cvt(a.cvt_type) == b.cvt_type
                && a.args.len() == b.args.len()
                && a.args.iter().zip(&b.args).all(|(x, y)| ast_equal(*x, *y))
        };
        if cancel {
            steps.remove(i + 1);
            steps.remove(i);
            changed = true;
            if i > 0 { i -= 1; }
            continue;
        }
        // Precession combination: Prec(e1,e2) + Prec(e2,e3) → Prec(e1,e3)
        let combine_prec = {
            let a = &steps[i];
            let b = &steps[i + 1];
            a.cvt_type == b.cvt_type
                && matches!(a.cvt_type, SlaCvtType::Prec | SlaCvtType::Prebn)
                && a.args.len() >= 2
                && b.args.len() >= 2
                && ast_equal(a.args[1], b.args[0])
        };
        if combine_prec {
            let e1 = steps[i].args[0];
            let e3 = steps[i + 1].args[1];
            if ast_equal(e1, e3) {
                // Prec(e,e) is identity — remove both
                steps.remove(i + 1);
                steps.remove(i);
            } else {
                steps[i].args[1] = e3;
                steps.remove(i + 1);
            }
            changed = true;
            if i > 0 { i -= 1; }
            continue;
        }
        i += 1;
    }
    changed
}
```

Then replace `try_merge`:

```rust
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
        let other_sla = other.as_any().downcast_ref::<SlaMap>()?;

        let self_steps = sla_normalized_steps(self, self_inv != self.inverted);
        let other_steps = sla_normalized_steps(other_sla, other_inv != other_sla.inverted);

        let mut steps = self_steps;
        steps.extend(other_steps);

        if !sla_simplify_steps(&mut steps) {
            return None;
        }

        if steps.is_empty() {
            Some(Box::new(UnitMap::new(2)))
        } else {
            let mut result = SlaMap::new();
            for cvt in steps {
                result.add(cvt.cvt_type, &cvt.args);
            }
            Some(Box::new(result))
        }
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_slamap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/slamap.rs crates/starlink-ast/tests/test_slamap.rs
git commit -m "SlaMap: implement step concatenation and simplification

Adjacent SlaMaps now merge by concatenating step lists and
simplifying (inverse pair cancellation, precession combination).
Follows the SpecMap pattern. Matches C slamap.c:2706-3194."
```

---

## Task 13: TimeMap step concatenation and simplification

**Files:**
- Modify: `crates/starlink-ast/src/mappings/timemap.rs:1066-1104`
- Test: `crates/starlink-ast/tests/test_timemap.rs`

- [ ] **Step 1: Write tests**

Add to `crates/starlink-ast/tests/test_timemap.rs`:

```rust
#[test]
fn timemap_merge_concatenates() {
    use starlink_ast::mappings::{Mapping, TimeCvtType, TimeMap};

    let mut tm1 = TimeMap::new();
    tm1.add(TimeCvtType::MjdToMjd, &[0.0, 51544.5]);
    let mut tm2 = TimeMap::new();
    tm2.add(TimeCvtType::TaiToUtc, &[]);

    let merged = tm1.try_merge(&tm2, true, false, false);
    assert!(merged.is_some());
}

#[test]
fn timemap_merge_cancels_inverse() {
    use starlink_ast::mappings::{Mapping, TimeCvtType, TimeMap, UnitMap};

    let mut tm1 = TimeMap::new();
    tm1.add(TimeCvtType::TaiToUtc, &[]);
    let mut tm2 = TimeMap::new();
    tm2.add(TimeCvtType::TaiToUtc, &[]);

    let merged = tm1.try_merge(&tm2, true, false, true);
    assert!(merged.is_some());
    assert!(merged.unwrap().as_any().is::<UnitMap>());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p starlink-ast --test test_timemap timemap_merge`
Expected: FAIL

- [ ] **Step 3: Implement TimeMap step concatenation**

Port the same pattern from SlaMap/SpecMap. Add `inverse_time_cvt()`, `time_normalized_steps()`, `time_simplify_steps()` helper functions and update `try_merge`. Follow the C reference `timemap.c:2099-2428` for the inverse-type mapping and no-effect-step elimination rules.

The implementation follows the same structure as SlaMap (Task 12) — normalize, concatenate, simplify, construct result.

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_timemap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/timemap.rs crates/starlink-ast/tests/test_timemap.rs
git commit -m "TimeMap: implement step concatenation and simplification

Adjacent TimeMaps merge by concatenating step lists with inverse pair
cancellation and no-effect step elimination. Matches C timemap.c:2099-2428."
```

---

## Task 14: SphMap relaxed cancellation conditions

**Files:**
- Modify: `crates/starlink-ast/src/mappings/sphmap.rs:195-223`
- Test: `crates/starlink-ast/tests/test_sphmap.rs`

- [ ] **Step 1: Write tests for relaxed cancellation**

Add to `crates/starlink-ast/tests/test_sphmap.rs`:

```rust
#[test]
fn sphmap_inv_fwd_cancels_without_unit_radius() {
    use starlink_ast::mappings::{Mapping, SphMap, UnitMap};

    // Inverse (sph->cart) followed by forward (cart->sph) should cancel
    // even without UnitRadius — palDcs2c always produces unit vectors.
    let s1 = SphMap::new(false, 0.0); // unit_radius=false
    let s2 = SphMap::new(false, 0.0); // unit_radius=false

    // self_inv=true (inverse first = sph->cart), other_inv=false (forward = cart->sph)
    let merged = s1.try_merge(&s2, true, true, false);
    assert!(merged.is_some());
    assert!(merged.unwrap().as_any().is::<UnitMap>());
}

#[test]
fn sphmap_fwd_inv_needs_unit_radius_on_first() {
    use starlink_ast::mappings::{Mapping, SphMap, UnitMap};

    // Forward (cart->sph) then inverse (sph->cart).
    // Requires UnitRadius on the first (forward) map only.
    let s1 = SphMap::new(true, 0.0);  // unit_radius=true (first, forward)
    let s2 = SphMap::new(false, 0.0); // unit_radius=false (second, inverse)

    let merged = s1.try_merge(&s2, true, false, true);
    assert!(merged.is_some());
    assert!(merged.unwrap().as_any().is::<UnitMap>());

    // Without UnitRadius on the first — should NOT cancel.
    let s3 = SphMap::new(false, 0.0); // no UnitRadius
    let merged2 = s3.try_merge(&s2, true, false, true);
    assert!(merged2.is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p starlink-ast --test test_sphmap sphmap_inv_fwd_cancels sphmap_fwd_inv_needs`
Expected: FAIL

- [ ] **Step 3: Fix SphMap try_merge**

In `crates/starlink-ast/src/mappings/sphmap.rs`, replace `try_merge`:

```rust
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
        if self_inv == other_inv {
            return None;
        }
        let other_sph = other.as_any().downcast_ref::<SphMap>()?;
        if !ast_equal(self.polar_long, other_sph.polar_long) {
            return None;
        }

        // Determine which direction each is applied.
        // SphMap forward: 3D Cartesian → 2D spherical (cart→sph)
        // SphMap inverse: 2D spherical → 3D Cartesian (sph→cart)
        let self_is_fwd = !self_inv;  // self applied forward (cart→sph)

        if self_is_fwd {
            // Forward→inverse (cart→sph, sph→cart):
            // Need UnitRadius on the first (forward) map only.
            if !self.unit_radius {
                return None;
            }
        }
        // Inverse→forward (sph→cart, cart→sph): no UnitRadius check needed.
        // palDcs2c always produces unit vectors.

        let self_nin = if self_inv { 2 } else { 3 };
        Some(Box::new(UnitMap::new(self_nin)))
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_sphmap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/sphmap.rs crates/starlink-ast/tests/test_sphmap.rs
git commit -m "SphMap: relax cancellation conditions

Inverse→forward cancels with only PolarLong equality check (no
UnitRadius needed). Forward→inverse requires UnitRadius on the
first map only. Matches C sphmap.c:748-761."
```

---

## Task 15: Analytical rate() for UnitMap, ZoomMap, ShiftMap

**Files:**
- Modify: `crates/starlink-ast/src/mappings/unit.rs:65-67`
- Modify: `crates/starlink-ast/src/mappings/zoom.rs:106-108`
- Modify: `crates/starlink-ast/src/mappings/shift.rs:98-100`
- Test: `crates/starlink-ast/tests/test_unitmap.rs`

- [ ] **Step 1: Write tests for analytical rates**

Add to `crates/starlink-ast/tests/test_unitmap.rs` (or create a new `test_rate.rs`):

```rust
#[test]
fn unit_rate_analytical() {
    use starlink_ast::mappings::{Mapping, UnitMap};
    let u = UnitMap::new(3);
    assert_eq!(u.rate(&[1.0, 2.0, 3.0], 1, 1).unwrap(), 1.0);
    assert_eq!(u.rate(&[1.0, 2.0, 3.0], 1, 2).unwrap(), 0.0);
    assert_eq!(u.rate(&[1.0, 2.0, 3.0], 2, 2).unwrap(), 1.0);
}

#[test]
fn zoom_rate_analytical() {
    use starlink_ast::mappings::{Mapping, ZoomMap};
    let zm = ZoomMap::new(2, 3.0).unwrap();
    assert_eq!(zm.rate(&[1.0, 2.0], 1, 1).unwrap(), 3.0);
    assert_eq!(zm.rate(&[1.0, 2.0], 1, 2).unwrap(), 0.0);

    let mut zm_inv = ZoomMap::new(2, 3.0).unwrap();
    zm_inv.invert();
    let r = zm_inv.rate(&[1.0, 2.0], 1, 1).unwrap();
    assert!((r - 1.0/3.0).abs() < 1e-15);
}

#[test]
fn shift_rate_analytical() {
    use starlink_ast::mappings::{Mapping, ShiftMap};
    let sm = ShiftMap::new(&[10.0, 20.0]);
    assert_eq!(sm.rate(&[1.0, 2.0], 1, 1).unwrap(), 1.0);
    assert_eq!(sm.rate(&[1.0, 2.0], 1, 2).unwrap(), 0.0);
}
```

- [ ] **Step 2: Run tests (they pass via numeric_rate but we want analytical)**

Run: `cargo test -p starlink-ast --test test_unitmap unit_rate`
Expected: PASS (numerically correct, but inefficient)

- [ ] **Step 3: Replace numeric_rate with analytical implementations**

In `crates/starlink-ast/src/mappings/unit.rs`:
```rust
    fn rate(&self, _at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        Ok(if ax1 == ax2 { 1.0 } else { 0.0 })
    }
```

In `crates/starlink-ast/src/mappings/zoom.rs`:
```rust
    fn rate(&self, _at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        if ax1 != ax2 {
            return Ok(0.0);
        }
        let fwd = !self.inverted;
        Ok(if fwd { self.factor } else { 1.0 / self.factor })
    }
```

In `crates/starlink-ast/src/mappings/shift.rs`:
```rust
    fn rate(&self, _at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        Ok(if ax1 == ax2 { 1.0 } else { 0.0 })
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_unitmap && cargo test -p starlink-ast --test test_zoommap && cargo test -p starlink-ast --test test_shiftmap`
Expected: PASS

- [ ] **Step 5: Remove unused numeric_rate imports**

Remove `use super::rate::numeric_rate;` from `unit.rs`, `zoom.rs`, `shift.rs` if now unused.

- [ ] **Step 6: Commit**

```bash
git add crates/starlink-ast/src/mappings/unit.rs crates/starlink-ast/src/mappings/zoom.rs crates/starlink-ast/src/mappings/shift.rs crates/starlink-ast/tests/test_unitmap.rs
git commit -m "Analytical rate() for UnitMap, ZoomMap, ShiftMap

Bypass numeric_rate() with known O(1) analytical values.
UnitMap/ShiftMap: rate=1 on diagonal, 0 off-diagonal.
ZoomMap: rate=factor (or 1/factor if inverted)."
```

---

## Task 16: Analytical rate() for MatrixMap and PermMap

**Files:**
- Modify: `crates/starlink-ast/src/mappings/matrix.rs:286-288`
- Modify: `crates/starlink-ast/src/mappings/perm.rs:302-304`
- Test: `crates/starlink-ast/tests/test_matrixmap.rs`

- [ ] **Step 1: Write tests**

Add to `crates/starlink-ast/tests/test_matrixmap.rs`:

```rust
#[test]
fn matrix_rate_analytical() {
    use starlink_ast::mappings::{Mapping, MatrixMap};
    use ndarray::array;

    let m = array![[1.0, 2.0], [3.0, 4.0]];
    let mm = MatrixMap::new(m).unwrap();
    // rate(ax1=output, ax2=input) = matrix element
    assert_eq!(mm.rate(&[0.0, 0.0], 1, 1).unwrap(), 1.0);
    assert_eq!(mm.rate(&[0.0, 0.0], 1, 2).unwrap(), 2.0);
    assert_eq!(mm.rate(&[0.0, 0.0], 2, 1).unwrap(), 3.0);
    assert_eq!(mm.rate(&[0.0, 0.0], 2, 2).unwrap(), 4.0);
}
```

Add to `crates/starlink-ast/tests/test_permmap.rs`:

```rust
#[test]
fn perm_rate_analytical() {
    use starlink_ast::mappings::{Mapping, PermMap};

    // outperm: [2, 1] => out[0]=in[1], out[1]=in[0]
    let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    // rate(output 1, input 1) = 0 (output 1 doesn't depend on input 1)
    assert_eq!(pm.rate(&[0.0, 0.0], 1, 1).unwrap(), 0.0);
    // rate(output 1, input 2) = 1 (output 1 = input 2)
    assert_eq!(pm.rate(&[0.0, 0.0], 1, 2).unwrap(), 1.0);
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p starlink-ast --test test_matrixmap matrix_rate && cargo test -p starlink-ast --test test_permmap perm_rate`
Expected: Currently pass via numeric approximation.

- [ ] **Step 3: Implement analytical rate()**

In `crates/starlink-ast/src/mappings/matrix.rs`:
```rust
    fn rate(&self, _at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        // Effective forward matrix element at [ax1-1, ax2-1].
        let m = if self.inverted {
            match &self.inverse {
                Some(inv) => inv,
                None => return Ok(BAD_VALUE),
            }
        } else {
            &self.matrix
        };
        let row = ax1 - 1;
        let col = ax2 - 1;
        if row < m.nrows() && col < m.ncols() {
            Ok(m[[row, col]])
        } else {
            Ok(0.0)
        }
    }
```

In `crates/starlink-ast/src/mappings/perm.rs`:
```rust
    fn rate(&self, _at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        let perm = if self.inverted { &self.inperm } else { &self.outperm };
        let idx = ax1 - 1;
        if idx < perm.len() && perm[idx] == ax2 as i64 {
            Ok(1.0)
        } else {
            Ok(0.0)
        }
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_matrixmap && cargo test -p starlink-ast --test test_permmap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/matrix.rs crates/starlink-ast/src/mappings/perm.rs crates/starlink-ast/tests/test_matrixmap.rs crates/starlink-ast/tests/test_permmap.rs
git commit -m "Analytical rate() for MatrixMap and PermMap

MatrixMap returns the effective forward matrix element.
PermMap returns 1.0 if the output reads from the queried input, else 0.0."
```

---

## Task 17: MatrixMap × PermMap merge

**Files:**
- Modify: `crates/starlink-ast/src/mappings/matrix.rs` (add case in `try_merge`)
- Test: `crates/starlink-ast/tests/test_matrixmap.rs`

- [ ] **Step 1: Write test**

Add to `crates/starlink-ast/tests/test_matrixmap.rs`:

```rust
#[test]
fn matrix_merge_with_permmap() {
    use starlink_ast::coords::PointSet;
    use starlink_ast::mappings::{Mapping, MatrixMap, PermMap};
    use ndarray::array;

    // Matrix [[1,2],[3,4]] followed by PermMap that swaps outputs.
    let m = array![[1.0, 2.0], [3.0, 4.0]];
    let mm = MatrixMap::new(m).unwrap();
    let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();

    let merged = mm.try_merge(&pm, true, false, false);
    assert!(merged.is_some());

    // Verify the merged mapping produces the same result.
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let expected_mat = mm.transform(&pts, true).unwrap();
    let expected = pm.transform(&expected_mat, true).unwrap();
    let actual = merged.unwrap().transform(&pts, true).unwrap();
    assert_eq!(actual.point(0), expected.point(0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p starlink-ast --test test_matrixmap matrix_merge_with_permmap`
Expected: FAIL — returns None.

- [ ] **Step 3: Add PermMap case to MatrixMap try_merge**

In `crates/starlink-ast/src/mappings/matrix.rs`, add after the ZoomMap case in `try_merge`:

```rust
        // MatrixMap + PermMap → MatrixMap (permuted rows/columns).
        if let Some(pm) = other.as_any().downcast_ref::<PermMap>() {
            let a = if self_inv {
                self.inverse.as_ref()?.clone()
            } else {
                self.matrix.clone()
            };
            // Convert PermMap to a permutation matrix.
            let perm = if other_inv { pm.inperm() } else { pm.outperm() };
            let n = perm.len();
            if a.nrows() != n {
                return None;
            }
            // Only handle pure permutations (positive indices, no constants).
            if perm.iter().any(|&p| p <= 0) {
                return None;
            }
            let mut perm_matrix = Array2::<f64>::zeros((n, n));
            for (row, &p) in perm.iter().enumerate() {
                perm_matrix[[row, (p as usize) - 1]] = 1.0;
            }
            // Result = perm_matrix * a (PermMap applied after MatrixMap)
            let product = perm_matrix.dot(&a);
            if is_identity_matrix(&product) {
                return Some(Box::new(UnitMap::new(n)));
            }
            return MatrixMap::new(product)
                .ok()
                .map(|m| Box::new(m) as DynMapping);
        }
```

Add `use ndarray::Array2;` if not already imported.

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_matrixmap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/matrix.rs crates/starlink-ast/tests/test_matrixmap.rs
git commit -m "MatrixMap: merge with adjacent PermMap

Converts PermMap to permutation matrix and multiplies.
Matches C matrixmap.c:2608 (MatPerm)."
```

---

## Task 18: Diagonal MatrixMap × WinMap merge

**Files:**
- Modify: `crates/starlink-ast/src/mappings/matrix.rs` (add case in `try_merge`)
- Test: `crates/starlink-ast/tests/test_matrixmap.rs`

- [ ] **Step 1: Write test**

Add to `crates/starlink-ast/tests/test_matrixmap.rs`:

```rust
#[test]
fn diagonal_matrix_merge_with_winmap() {
    use starlink_ast::coords::PointSet;
    use starlink_ast::mappings::{Mapping, MatrixMap, WinMap};
    use ndarray::array;

    // Diagonal matrix [2,3] followed by WinMap shift=[1,1] scale=[1,1]
    let m = array![[2.0, 0.0], [0.0, 3.0]];
    let mm = MatrixMap::new(m).unwrap();
    let wm = WinMap::new(&[1.0, 1.0], &[1.0, 1.0]).unwrap();

    let merged = mm.try_merge(&wm, true, false, false);
    assert!(merged.is_some());

    // Verify: matrix then winmap = (shift + scale * (diag * input))
    let pts = PointSet::from_rows([[5.0, 7.0]]);
    let mat_out = mm.transform(&pts, true).unwrap();
    let expected = wm.transform(&mat_out, true).unwrap();
    let actual = merged.unwrap().transform(&pts, true).unwrap();
    assert_eq!(actual.point(0)[0], expected.point(0)[0]);
    assert_eq!(actual.point(0)[1], expected.point(0)[1]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p starlink-ast --test test_matrixmap diagonal_matrix_merge_with_winmap`
Expected: FAIL

- [ ] **Step 3: Add WinMap case to MatrixMap try_merge**

In `crates/starlink-ast/src/mappings/matrix.rs`, add to `try_merge`:

```rust
        // Diagonal MatrixMap + WinMap → WinMap (absorb diagonal scales).
        if let Some(wm) = other.as_any().downcast_ref::<WinMap>() {
            let a = if self_inv {
                self.inverse.as_ref()?.clone()
            } else {
                self.matrix.clone()
            };
            if !is_diagonal(&a) {
                return None;
            }
            let n = a.nrows();
            if n != wm.nin() {
                return None;
            }
            let (wshifts, wscales) = winmap_effective_params(wm.shifts(), wm.scales(), other_inv);
            // Combined: out = wshift + wscale * (diag * x) = wshift + (wscale*diag) * x
            let mut new_shifts = Vec::with_capacity(n);
            let mut new_scales = Vec::with_capacity(n);
            for i in 0..n {
                let d = a[[i, i]];
                new_scales.push(wscales[i] * d);
                new_shifts.push(wshifts[i]);
            }
            return WinMap::new(&new_shifts, &new_scales)
                .ok()
                .map(|m| Box::new(m) as DynMapping);
        }
```

Add `use super::winmap::winmap_effective_params;` or make that function `pub(crate)` if needed.

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_matrixmap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/matrix.rs crates/starlink-ast/tests/test_matrixmap.rs
git commit -m "MatrixMap: merge diagonal with adjacent WinMap

Absorbs per-axis diagonal scales into WinMap scale factors.
Matches C matrixmap.c:3230 (MatWin2)."
```

---

## Task 19: WinMap parallel merging

**Files:**
- Modify: `crates/starlink-ast/src/mappings/winmap.rs:231` (the `!series` path)
- Test: `crates/starlink-ast/tests/test_winmap.rs`

- [ ] **Step 1: Write test**

Add to `crates/starlink-ast/tests/test_winmap.rs`:

```rust
#[test]
fn winmap_parallel_merge() {
    use starlink_ast::coords::PointSet;
    use starlink_ast::mappings::{CmpMap, Composition, Mapping, WinMap};

    let w1 = WinMap::new(&[1.0, 2.0], &[3.0, 4.0]).unwrap();
    let w2 = WinMap::new(&[5.0], &[6.0]).unwrap();

    // Parallel merge: result should be a 3-axis WinMap.
    let merged = w1.try_merge(&w2, false, false, false);
    assert!(merged.is_some());
    let m = merged.unwrap();
    assert_eq!(m.nin(), 3);
    assert_eq!(m.nout(), 3);

    // Verify output matches parallel composition.
    let pts = PointSet::from_rows([[1.0, 2.0, 3.0]]);
    let actual = m.transform(&pts, true).unwrap();
    // w1 applied to [1,2]: [1+3*1, 2+4*2] = [4, 10]
    // w2 applied to [3]: [5+6*3] = [23]
    assert_eq!(actual.point(0)[0], 4.0);
    assert_eq!(actual.point(0)[1], 10.0);
    assert_eq!(actual.point(0)[2], 23.0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p starlink-ast --test test_winmap winmap_parallel_merge`
Expected: FAIL

- [ ] **Step 3: Implement parallel merge in WinMap try_merge**

In `crates/starlink-ast/src/mappings/winmap.rs`, replace the `if !series { return None; }` block:

```rust
        if !series {
            // Parallel: concatenate axis terms.
            let (s1, c1) = winmap_effective_params(&self.shifts, &self.scales, self_inv);
            let (s2, c2) = extract_linear_params(other, other_inv)?;
            let mut new_shifts = s1;
            new_shifts.extend(s2);
            let mut new_scales = c1;
            new_scales.extend(c2);
            return WinMap::new(&new_shifts, &new_scales)
                .ok()
                .map(|m| Box::new(m) as DynMapping);
        }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_winmap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/winmap.rs crates/starlink-ast/tests/test_winmap.rs
git commit -m "WinMap: implement parallel merging

Adjacent parallel WinMaps (and WinMap+ZoomMap, WinMap+ShiftMap)
merge by concatenating axis terms. Matches C winmap.c:1697-1813."
```

---

## Task 20: WinMap + diagonal MatrixMap series merge

**Files:**
- Modify: `crates/starlink-ast/src/mappings/winmap.rs` (`extract_linear_params`)
- Test: `crates/starlink-ast/tests/test_winmap.rs`

- [ ] **Step 1: Write test**

Add to `crates/starlink-ast/tests/test_winmap.rs`:

```rust
#[test]
fn winmap_merge_with_diagonal_matrix() {
    use starlink_ast::coords::PointSet;
    use starlink_ast::mappings::{Mapping, MatrixMap, WinMap};
    use ndarray::array;

    // WinMap [shift=1, scale=2] followed by diagonal MatrixMap [3, 0; 0, 4]
    let wm = WinMap::new(&[1.0, 1.0], &[2.0, 2.0]).unwrap();
    let m = array![[3.0, 0.0], [0.0, 4.0]];
    let mm = MatrixMap::new(m).unwrap();

    let merged = wm.try_merge(&mm, true, false, false);
    assert!(merged.is_some());

    let pts = PointSet::from_rows([[5.0, 7.0]]);
    let win_out = wm.transform(&pts, true).unwrap();
    let expected = mm.transform(&win_out, true).unwrap();
    let actual = merged.unwrap().transform(&pts, true).unwrap();
    assert_eq!(actual.point(0)[0], expected.point(0)[0]);
    assert_eq!(actual.point(0)[1], expected.point(0)[1]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p starlink-ast --test test_winmap winmap_merge_with_diagonal_matrix`
Expected: FAIL

- [ ] **Step 3: Extend extract_linear_params to recognize diagonal MatrixMap**

In `crates/starlink-ast/src/mappings/winmap.rs`, add to `extract_linear_params`:

```rust
    if let Some(mm) = mapping.as_any().downcast_ref::<MatrixMap>() {
        let m = if inv {
            mm.inverse_matrix()?
        } else {
            mm.forward_matrix()
        };
        if !is_diagonal(m) {
            return None;
        }
        let n = m.nrows();
        let shifts = vec![0.0; n];
        let scales: Vec<f64> = (0..n).map(|i| m[[i, i]]).collect();
        return Some((shifts, scales));
    }
```

This requires adding public accessor methods `forward_matrix()` and `inverse_matrix()` to `MatrixMap` (returning `&Array2<f64>` and `Option<&Array2<f64>>`), and making `is_diagonal` accessible from the winmap module.

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_winmap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/winmap.rs crates/starlink-ast/src/mappings/matrix.rs crates/starlink-ast/tests/test_winmap.rs
git commit -m "WinMap: merge with diagonal MatrixMap in series

Extends extract_linear_params to recognize diagonal MatrixMap,
enabling composition into a single WinMap.
Matches C winmap.c:1164-1229."
```

---

## Task 21: WinMap self-simplification to ShiftMap/MatrixMap

**Files:**
- Modify: `crates/starlink-ast/src/mappings/winmap.rs:197-199`
- Test: `crates/starlink-ast/tests/test_winmap.rs`

- [ ] **Step 1: Write test**

Add to `crates/starlink-ast/tests/test_winmap.rs`:

```rust
#[test]
fn winmap_simplifies_to_shiftmap() {
    use starlink_ast::mappings::{Mapping, ShiftMap, WinMap};

    // All scales = 1.0 → should simplify to ShiftMap
    let wm = WinMap::new(&[3.0, 5.0], &[1.0, 1.0]).unwrap();
    let s = wm.simplified().unwrap();
    assert!(s.as_any().is::<ShiftMap>());
}

#[test]
fn winmap_simplifies_to_zoommap() {
    use starlink_ast::mappings::{Mapping, WinMap, ZoomMap};

    // All shifts = 0.0, uniform scale → should simplify to ZoomMap
    let wm = WinMap::new(&[0.0, 0.0], &[4.0, 4.0]).unwrap();
    let s = wm.simplified().unwrap();
    assert!(s.as_any().is::<ZoomMap>());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p starlink-ast --test test_winmap winmap_simplifies`
Expected: FAIL — `simplified()` returns self.

- [ ] **Step 3: Implement WinMap simplified()**

In `crates/starlink-ast/src/mappings/winmap.rs`, replace `simplified()`:

```rust
    fn simplified(&self) -> AstResult<DynMapping> {
        let n = self.shifts.len();
        let all_shifts_zero = self.shifts.iter().all(|&s| ast_equal(s, 0.0));
        let all_scales_one = self.scales.iter().all(|&s| ast_equal(s, 1.0));

        // Identity
        if all_shifts_zero && all_scales_one {
            return Ok(Box::new(UnitMap::new(n)));
        }
        // Pure shift (all scales = 1)
        if all_scales_one {
            let shifts: Vec<f64> = if self.inverted {
                self.shifts.iter().map(|&s| -s).collect()
            } else {
                self.shifts.clone()
            };
            return Ok(Box::new(ShiftMap::new(&shifts)));
        }
        // Pure uniform zoom (all shifts = 0, all scales equal)
        if all_shifts_zero {
            let first = self.scales[0];
            if (1..n).all(|i| ast_equal(self.scales[i], first)) {
                let f = if self.inverted { 1.0 / first } else { first };
                if let Ok(zm) = ZoomMap::new(n, f) {
                    return Ok(Box::new(zm));
                }
            }
        }
        Ok(Box::new(self.clone()))
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_winmap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/winmap.rs crates/starlink-ast/tests/test_winmap.rs
git commit -m "WinMap: self-simplify to ShiftMap/ZoomMap/UnitMap

All-unity-scale WinMap → ShiftMap. All-zero-shift uniform-scale
WinMap → ZoomMap. Identity → UnitMap. Enables downstream merges
with different partner types. Matches C winmap.c:1082-1131."
```

---

## Task 22: SpecMap unconditional adjacent merge

**Files:**
- Modify: `crates/starlink-ast/src/mappings/specmap.rs:1164`
- Test: `crates/starlink-ast/tests/test_specmap.rs`

- [ ] **Step 1: Write test**

Add to `crates/starlink-ast/tests/test_specmap.rs`:

```rust
#[test]
fn specmap_always_merges_adjacent() {
    use starlink_ast::mappings::{Mapping, SpecCvtType, SpecMap};

    // Two SpecMaps with non-cancelling steps should still merge.
    let mut sm1 = SpecMap::new();
    sm1.add(SpecCvtType::FrToWv, &[]);
    let mut sm2 = SpecMap::new();
    sm2.add(SpecCvtType::VrToVo, &[1.0]);

    let merged = sm1.try_merge(&sm2, true, false, false);
    assert!(merged.is_some(), "Adjacent SpecMaps should always merge");
    // Result should be a SpecMap with both steps.
    assert!(merged.unwrap().as_any().is::<SpecMap>());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p starlink-ast --test test_specmap specmap_always_merges`
Expected: FAIL — current code only merges if simplification occurs.

- [ ] **Step 3: Fix SpecMap try_merge to always merge**

In `crates/starlink-ast/src/mappings/specmap.rs`, change the `try_merge` logic. Replace:

```rust
        if !simplify_steps(&mut steps) {
            return None;
        }
```

With:

```rust
        simplify_steps(&mut steps);
```

(Remove the early return — always produce the merged result.)

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_specmap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/specmap.rs crates/starlink-ast/tests/test_specmap.rs
git commit -m "SpecMap: always merge adjacent SpecMaps

Concatenate step lists even when no cancellation occurs. Enables
further simplification when a third SpecMap appears.
Matches C specmap.c:2259-2288."
```

---

## Task 23: ZoomMap parallel merge → diagonal MatrixMap

**Files:**
- Modify: `crates/starlink-ast/src/mappings/zoom.rs` (`try_merge`)
- Test: `crates/starlink-ast/tests/test_zoommap.rs`

- [ ] **Step 1: Write test**

Add to `crates/starlink-ast/tests/test_zoommap.rs`:

```rust
#[test]
fn zoom_parallel_merge_to_matrix() {
    use starlink_ast::coords::PointSet;
    use starlink_ast::mappings::{Mapping, MatrixMap, ZoomMap};

    let z1 = ZoomMap::new(1, 2.0).unwrap();
    let z2 = ZoomMap::new(1, 3.0).unwrap();

    let merged = z1.try_merge(&z2, false, false, false);
    assert!(merged.is_some());
    let m = merged.unwrap();
    // Different factors → diagonal MatrixMap
    assert!(m.as_any().is::<MatrixMap>());
    assert_eq!(m.nin(), 2);

    let pts = PointSet::from_rows([[5.0, 7.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 10.0);
    assert_eq!(out.point(0)[1], 21.0);
}

#[test]
fn zoom_parallel_merge_same_factor() {
    use starlink_ast::mappings::{Mapping, ZoomMap};

    let z1 = ZoomMap::new(2, 5.0).unwrap();
    let z2 = ZoomMap::new(1, 5.0).unwrap();

    let merged = z1.try_merge(&z2, false, false, false);
    assert!(merged.is_some());
    // Same factor → should be a ZoomMap with ncoord=3
    let m = merged.unwrap();
    assert!(m.as_any().is::<ZoomMap>());
    assert_eq!(m.nin(), 3);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p starlink-ast --test test_zoommap zoom_parallel`
Expected: FAIL

- [ ] **Step 3: Add parallel merge to ZoomMap try_merge**

In `crates/starlink-ast/src/mappings/zoom.rs`, update `try_merge` (currently returns None for `!series`):

```rust
    fn try_merge(
        &self,
        other: &dyn Mapping,
        series: bool,
        self_inv: bool,
        other_inv: bool,
    ) -> Option<DynMapping> {
        let other_zoom = other.as_any().downcast_ref::<ZoomMap>()?;

        if !series {
            // Parallel: combine into diagonal MatrixMap or wider ZoomMap.
            let f1 = if self_inv { 1.0 / self.factor } else { self.factor };
            let f2 = if other_inv { 1.0 / other_zoom.factor } else { other_zoom.factor };
            let n = self.ncoord + other_zoom.ncoord;
            if ast_equal(f1, f2) {
                return ZoomMap::new(n, f1).ok().map(|z| Box::new(z) as DynMapping);
            }
            // Different factors → diagonal MatrixMap
            let mut diag = vec![0.0; n * n];
            for i in 0..self.ncoord {
                diag[i * n + i] = f1;
            }
            for i in 0..other_zoom.ncoord {
                let idx = self.ncoord + i;
                diag[idx * n + idx] = f2;
            }
            let m = ndarray::Array2::from_shape_vec((n, n), diag).ok()?;
            return MatrixMap::new(m).ok().map(|mm| Box::new(mm) as DynMapping);
        }

        // Series: existing logic...
        // (keep existing series merge code unchanged)
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_zoommap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/zoom.rs crates/starlink-ast/tests/test_zoommap.rs
git commit -m "ZoomMap: parallel merge → diagonal MatrixMap

Same factor → wider ZoomMap. Different factors → diagonal MatrixMap.
Matches C zoommap.c:823-938."
```

---

## Task 24: PermMap simplified() checks inperm

**Files:**
- Modify: `crates/starlink-ast/src/mappings/perm.rs:287-299`
- Test: `crates/starlink-ast/tests/test_permmap.rs`

- [ ] **Step 1: Write test**

Add to `crates/starlink-ast/tests/test_permmap.rs`:

```rust
#[test]
fn perm_simplified_checks_both_perms() {
    use starlink_ast::mappings::{Mapping, PermMap, UnitMap};

    // Identity outperm but non-identity inperm → NOT a UnitMap.
    let pm = PermMap::new(&[2, 1], &[1, 2], &[]).unwrap();
    let s = pm.simplified().unwrap();
    assert!(!s.as_any().is::<UnitMap>());

    // Both identity → IS a UnitMap.
    let pm2 = PermMap::new(&[1, 2], &[1, 2], &[]).unwrap();
    let s2 = pm2.simplified().unwrap();
    assert!(s2.as_any().is::<UnitMap>());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p starlink-ast --test test_permmap perm_simplified_checks_both`
Expected: FAIL — first assert fails (incorrectly simplifies to UnitMap).

- [ ] **Step 3: Fix PermMap simplified()**

In `crates/starlink-ast/src/mappings/perm.rs`, replace `simplified()`:

```rust
    fn simplified(&self) -> AstResult<DynMapping> {
        let n = self.outperm.len();
        let outperm_is_identity = n == self.inperm.len()
            && self.outperm.iter().enumerate().all(|(i, &p)| p == (i as i64 + 1));
        let inperm_is_identity = self.inperm.iter().enumerate().all(|(i, &p)| p == (i as i64 + 1));
        if outperm_is_identity && inperm_is_identity {
            Ok(Box::new(UnitMap::new(n)))
        } else {
            Ok(Box::new(self.clone()))
        }
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_permmap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/perm.rs crates/starlink-ast/tests/test_permmap.rs
git commit -m "PermMap: simplified() checks both inperm and outperm

Prevents incorrect simplification to UnitMap when only outperm is
identity but inperm is not. Matches C permmap.c:1283-1333."
```

---

## Task 25: MatrixMap full→diagonal self-simplification

**Files:**
- Modify: `crates/starlink-ast/src/mappings/matrix.rs:187-213`
- Test: `crates/starlink-ast/tests/test_matrixmap.rs`

- [ ] **Step 1: Write test**

Add to `crates/starlink-ast/tests/test_matrixmap.rs`:

```rust
#[test]
fn matrix_full_to_diagonal_simplification() {
    use starlink_ast::mappings::{Mapping, MatrixMap, ZoomMap};
    use ndarray::array;

    // A full matrix that happens to be diagonal with non-uniform elements.
    let m = array![[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 4.0]];
    let mm = MatrixMap::new(m).unwrap();
    let s = mm.simplified().unwrap();
    // Non-uniform diagonal: stays MatrixMap (but conceptually "diagonal form").
    // Uniform diagonal → ZoomMap.
    let m2 = array![[5.0, 0.0], [0.0, 5.0]];
    let mm2 = MatrixMap::new(m2).unwrap();
    let s2 = mm2.simplified().unwrap();
    assert!(s2.as_any().is::<ZoomMap>());
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p starlink-ast --test test_matrixmap matrix_full_to_diagonal`
Expected: PASS (the uniform diagonal case already works).

This is already handled. The existing `simplified()` already converts uniform diagonal to ZoomMap and identity to UnitMap. The non-uniform diagonal case stays as MatrixMap (which is correct for now — the internal form optimization is Task 25b below for performance, not correctness). Mark complete.

- [ ] **Step 3: Commit (no changes needed if tests already pass)**

If tests pass, this task is already complete. Move to next task.

---

## Task 26: PermMap parallel merge

**Files:**
- Modify: `crates/starlink-ast/src/mappings/perm.rs:306-313`
- Test: `crates/starlink-ast/tests/test_permmap.rs`

- [ ] **Step 1: Write test**

Add to `crates/starlink-ast/tests/test_permmap.rs`:

```rust
#[test]
fn perm_parallel_merge() {
    use starlink_ast::coords::PointSet;
    use starlink_ast::mappings::{Mapping, PermMap};

    // PermMap [2,1] parallel with PermMap [1] → PermMap [2,1,3]
    let p1 = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let p2 = PermMap::new(&[1], &[1], &[]).unwrap();

    let merged = p1.try_merge(&p2, false, false, false);
    assert!(merged.is_some());
    let m = merged.unwrap();
    assert_eq!(m.nin(), 3);
    assert_eq!(m.nout(), 3);

    // Verify: input [a, b, c] → [b, a, c]
    let pts = PointSet::from_rows([[1.0, 2.0, 3.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 2.0);
    assert_eq!(out.point(0)[1], 1.0);
    assert_eq!(out.point(0)[2], 3.0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p starlink-ast --test test_permmap perm_parallel_merge`
Expected: FAIL

- [ ] **Step 3: Add parallel merge to PermMap try_merge**

In `crates/starlink-ast/src/mappings/perm.rs`, change `if !series { return None; }` to:

```rust
        if !series {
            // Parallel: offset-adjust and concatenate.
            let n1_out = self.outperm.len();
            let n1_in = self.inperm.len();

            let self_outperm = if self_inv { &self.inperm } else { &self.outperm };
            let self_inperm = if self_inv { &self.outperm } else { &self.inperm };
            let other_outperm = if other_inv { &other_perm.inperm } else { &other_perm.outperm };
            let other_inperm = if other_inv { &other_perm.outperm } else { &other_perm.inperm };

            let n1_eff_in = self_inperm.len();
            let n1_eff_out = self_outperm.len();

            let mut merged_constants: Vec<f64> = Vec::new();

            // Build combined outperm: first n1_eff_out from self, then offset others.
            let mut new_outperm: Vec<i64> = Vec::new();
            for &p in self_outperm {
                if p > 0 {
                    new_outperm.push(p); // same input index
                } else if p < 0 {
                    let val = self.constants[(-p - 1) as usize];
                    let idx = add_constant(&mut merged_constants, val);
                    new_outperm.push(-((idx + 1) as i64));
                } else {
                    new_outperm.push(0);
                }
            }
            for &p in other_outperm {
                if p > 0 {
                    new_outperm.push(p + n1_eff_in as i64); // offset
                } else if p < 0 {
                    let val = other_perm.constants[(-p - 1) as usize];
                    let idx = add_constant(&mut merged_constants, val);
                    new_outperm.push(-((idx + 1) as i64));
                } else {
                    new_outperm.push(0);
                }
            }

            // Build combined inperm similarly.
            let mut new_inperm: Vec<i64> = Vec::new();
            for &p in self_inperm {
                if p > 0 {
                    new_inperm.push(p); // same output index
                } else if p < 0 {
                    let val = self.constants[(-p - 1) as usize];
                    let idx = add_constant(&mut merged_constants, val);
                    new_inperm.push(-((idx + 1) as i64));
                } else {
                    new_inperm.push(0);
                }
            }
            for &p in other_inperm {
                if p > 0 {
                    new_inperm.push(p + n1_eff_out as i64); // offset
                } else if p < 0 {
                    let val = other_perm.constants[(-p - 1) as usize];
                    let idx = add_constant(&mut merged_constants, val);
                    new_inperm.push(-((idx + 1) as i64));
                } else {
                    new_inperm.push(0);
                }
            }

            return PermMap::new(&new_inperm, &new_outperm, &merged_constants)
                .ok()
                .map(|pm| Box::new(pm) as DynMapping);
        }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_permmap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/perm.rs crates/starlink-ast/tests/test_permmap.rs
git commit -m "PermMap: implement parallel merge

Offset-adjusts indices and concatenates into a single larger PermMap.
Matches C permmap.c:1088-1280."
```

---

## Task 27: LutMap inverse self-cancellation

**Files:**
- Modify: `crates/starlink-ast/src/mappings/lutmap.rs`
- Test: `crates/starlink-ast/tests/test_lutmap.rs`

- [ ] **Step 1: Write test**

Add to `crates/starlink-ast/tests/test_lutmap.rs`:

```rust
#[test]
fn lutmap_cancels_with_inverse() {
    use starlink_ast::mappings::{LutInterp, LutMap, Mapping, UnitMap};

    let lut = vec![0.0, 1.0, 4.0, 9.0, 16.0];
    let lm = LutMap::new(&lut, 0.0, 1.0, LutInterp::Linear).unwrap();

    // Same LUT, opposite invert → should cancel to UnitMap.
    let merged = lm.try_merge(&lm, true, false, true);
    assert!(merged.is_some());
    assert!(merged.unwrap().as_any().is::<UnitMap>());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p starlink-ast --test test_lutmap lutmap_cancels`
Expected: FAIL — no `try_merge` override.

- [ ] **Step 3: Implement LutMap try_merge**

Add to `crates/starlink-ast/src/mappings/lutmap.rs` in the `Mapping` impl:

```rust
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
        if self_inv == other_inv {
            return None;
        }
        let other_lut = other.as_any().downcast_ref::<LutMap>()?;
        // Cancel if same LUT (same data, start, inc, interp).
        if self.lut.len() != other_lut.lut.len() {
            return None;
        }
        if !ast_equal(self.start, other_lut.start) || !ast_equal(self.inc, other_lut.inc) {
            return None;
        }
        if self.interp != other_lut.interp {
            return None;
        }
        if !self.lut.iter().zip(&other_lut.lut).all(|(a, b)| ast_equal(*a, *b)) {
            return None;
        }
        Some(Box::new(UnitMap::new(1)))
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_lutmap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/lutmap.rs crates/starlink-ast/tests/test_lutmap.rs
git commit -m "LutMap: implement try_merge for inverse self-cancellation

Same LUT in opposite directions cancels to UnitMap.
Matches C lutmap.c:1174-1232."
```

---

## Task 28: SlaMap palMappa hoisting

**Files:**
- Modify: `crates/starlink-ast/src/mappings/slamap.rs` (around line 790)
- Test: (performance — verify correctness via existing tests)

- [ ] **Step 1: Identify the per-point palMappa call**

Read `slamap.rs` around line 790 to find where `palMappa` is called inside the per-point loop for AMP/MAP conversions.

- [ ] **Step 2: Hoist the computation**

Move the `palMappa` call (which computes star-independent parameters for mean→apparent or apparent→mean conversion) outside the per-point loop. Store the result in a local variable and pass it to the per-point computation.

The C reference (`slamap.c:242-246`) caches this in a static variable per SlaMap; in Rust, computing once at the top of `transform_into` for AMP/MAP steps is sufficient.

- [ ] **Step 3: Run existing tests to verify no regression**

Run: `cargo test -p starlink-ast --test test_slamap`
Expected: PASS (same results, just faster)

- [ ] **Step 4: Commit**

```bash
git add crates/starlink-ast/src/mappings/slamap.rs
git commit -m "SlaMap: hoist palMappa outside per-point loop

Compute star-independent parameters once per transform_into call
rather than per point. Matches C slamap.c:242-246 caching pattern."
```

---

## Task 29: CmpMap re-expansion after merge

**Files:**
- Modify: `crates/starlink-ast/src/mappings/simplify.rs:102-175`
- Test: `crates/starlink-ast/tests/test_simplify_regression.rs`

- [ ] **Step 1: Write test for re-expansion**

Add to `crates/starlink-ast/tests/test_simplify_regression.rs`:

```rust
#[test]
fn cmpmap_re_expands_after_merge() {
    use starlink_ast::mappings::{Mapping, ShiftMap, WinMap, ZoomMap, CmpMap, Composition, UnitMap};

    // A ; B ; C where merging A;B produces a WinMap, which should then
    // merge with C. Without re-expansion, the merged result (WinMap)
    // stays as-is and the merge with C is attempted but the
    // intermediate CmpMap blocks it.
    let a = ShiftMap::new(&[1.0]);
    let b = ZoomMap::new(1, 2.0).unwrap();
    let c = ShiftMap::new(&[3.0]);

    let ab = CmpMap::new(Box::new(a), Box::new(b), Composition::Series).unwrap();
    let abc = CmpMap::new(Box::new(ab), Box::new(c), Composition::Series).unwrap();

    let s = abc.simplified().unwrap();
    // Should reduce to a single WinMap (shift=2*1+3=5, scale=2).
    assert!(s.as_any().is::<WinMap>());
}
```

- [ ] **Step 2: Run test**

Run: `cargo test -p starlink-ast --test test_simplify_regression cmpmap_re_expands`
Expected: This likely already passes due to flattening. If it does, write a test that specifically exercises the re-expansion case where a merge produces a CmpMap of matching composition.

- [ ] **Step 3: Implement re-expansion in merge_loop**

In `crates/starlink-ast/src/mappings/simplify.rs`, after a successful merge at line 127, check if the merged result is a CmpMap of the matching composition kind. If so, flatten it and splice its children into the list at position `i`:

```rust
            if let Some(m) = merged {
                // Check if merged result is a CmpMap that should be re-expanded.
                if let Some(cmp) = m.as_any().downcast_ref::<CmpMap>() {
                    let target_kind = if series { Composition::Series } else { Composition::Parallel };
                    if cmp.composition() == target_kind {
                        if let Ok(expanded) = flatten(m.as_ref(), false, series) {
                            list.remove(i + 1);
                            list.remove(i);
                            for (idx, item) in expanded.into_iter().enumerate() {
                                list.insert(i + idx, item);
                            }
                            improved = true;
                            continue;
                        }
                    }
                }
                list[i] = (m, false);
                list.remove(i + 1);
                improved = true;
            } else {
                i += 1;
            }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_simplify_regression`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/simplify.rs crates/starlink-ast/tests/test_simplify_regression.rs
git commit -m "CmpMap simplification: re-expand merged CmpMaps

When a merge produces a CmpMap of matching composition, expand its
children back into the flat list for further merging.
Matches C cmpmap.c:1471-1525 (Strategy A)."
```

---

## Task 30: PolyMap coefficient consolidation

**Files:**
- Modify: `crates/starlink-ast/src/mappings/polymap.rs` (`simplified()`)
- Test: `crates/starlink-ast/tests/test_polymap.rs`

- [ ] **Step 1: Write test**

Add to `crates/starlink-ast/tests/test_polymap.rs`:

```rust
#[test]
fn polymap_consolidates_duplicate_terms() {
    use starlink_ast::mappings::{Mapping, PolyMap, PolyTerm};

    // Two terms with the same power combination: 2*x + 3*x → 5*x
    let terms = vec![
        PolyTerm { coeff: 2.0, powers: vec![1], out_axis: 1 },
        PolyTerm { coeff: 3.0, powers: vec![1], out_axis: 1 },
    ];
    let pm = PolyMap::new(1, 1, &terms, &[]).unwrap();
    let s = pm.simplified().unwrap();

    // Verify it produces the correct result (5*x).
    use starlink_ast::coords::PointSet;
    let pts = PointSet::from_rows([[7.0]]);
    let out = s.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 35.0);
}
```

- [ ] **Step 2: Run test**

Run: `cargo test -p starlink-ast --test test_polymap polymap_consolidates`
Expected: May pass (sum gives correct answer even without consolidation). Write a test that checks the number of terms instead:

```rust
#[test]
fn polymap_removes_zero_terms() {
    use starlink_ast::mappings::{Mapping, PolyMap, PolyTerm};

    // Two terms that cancel: 2*x + (-2)*x → 0 → should simplify
    let terms = vec![
        PolyTerm { coeff: 2.0, powers: vec![1], out_axis: 1 },
        PolyTerm { coeff: -2.0, powers: vec![1], out_axis: 1 },
    ];
    let pm = PolyMap::new(1, 1, &terms, &[]).unwrap();
    let s = pm.simplified().unwrap();

    use starlink_ast::coords::PointSet;
    let pts = PointSet::from_rows([[7.0]]);
    let out = s.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 0.0);
}
```

- [ ] **Step 3: Implement PolyMap simplified()**

In `crates/starlink-ast/src/mappings/polymap.rs`, replace the no-op `simplified()`:

```rust
    fn simplified(&self) -> AstResult<DynMapping> {
        // Consolidate terms: merge duplicates, remove zeros.
        let mut consolidated: Vec<PolyTerm> = Vec::new();
        for term in &self.forward {
            if let Some(existing) = consolidated.iter_mut().find(|t| {
                t.out_axis == term.out_axis && t.powers == term.powers
            }) {
                existing.coeff += term.coeff;
            } else {
                consolidated.push(term.clone());
            }
        }
        // Remove zero-coefficient terms.
        consolidated.retain(|t| t.coeff.abs() > 1e-30);

        if consolidated.len() == self.forward.len() {
            return Ok(Box::new(self.clone()));
        }
        if consolidated.is_empty() {
            return Ok(Box::new(UnitMap::new(self.nin)));
        }
        let inv = self.inverse.as_deref().unwrap_or(&[]);
        PolyMap::new(self.nin, self.nout, &consolidated, inv)
            .map(|pm| Box::new(pm) as DynMapping)
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_polymap`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/polymap.rs crates/starlink-ast/tests/test_polymap.rs
git commit -m "PolyMap: consolidate duplicate terms in simplified()

Merges terms with identical power combinations by summing coefficients
and removes zero-valued terms. Matches C polymap.c:3606-3717."
```

---

## Task 31: LutMap Inf handling

**Files:**
- Modify: `crates/starlink-ast/src/mappings/helpers.rs:6`
- Modify: `crates/starlink-ast/src/mappings/lutmap.rs` (constructor)
- Test: `crates/starlink-ast/tests/test_lutmap.rs`

- [ ] **Step 1: Write test**

Add to `crates/starlink-ast/tests/test_lutmap.rs`:

```rust
#[test]
fn lutmap_treats_inf_as_bad() {
    use starlink_ast::constants::BAD_VALUE;
    use starlink_ast::coords::PointSet;
    use starlink_ast::mappings::{LutInterp, LutMap, Mapping};

    let pts = PointSet::from_rows([[f64::INFINITY]]);
    let lut = vec![0.0, 1.0, 2.0, 3.0];
    let lm = LutMap::new(&lut, 0.0, 1.0, LutInterp::Linear).unwrap();
    let out = lm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
}

#[test]
fn is_bad_catches_inf() {
    use starlink_ast::mappings::helpers::is_bad;
    assert!(is_bad(f64::INFINITY));
    assert!(is_bad(f64::NEG_INFINITY));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p starlink-ast --test test_lutmap lutmap_treats_inf`
Expected: FAIL

- [ ] **Step 3: Fix is_bad to catch Inf**

In `crates/starlink-ast/src/mappings/helpers.rs`:

```rust
#[inline]
pub fn is_bad(v: f64) -> bool {
    v == BAD_VALUE || !v.is_finite()
}
```

This replaces `v.is_nan()` with `!v.is_finite()` which catches both NaN and Inf.

- [ ] **Step 4: Run full test suite to check for regressions**

Run: `cargo test -p starlink-ast`
Expected: PASS (no regressions — Inf was never a valid coordinate)

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/helpers.rs crates/starlink-ast/tests/test_lutmap.rs
git commit -m "is_bad: catch Inf values as well as NaN

Extend is_bad() to treat ±Infinity as BAD. No valid astronomical
coordinate should be Inf."
```

---

## Task 32: DoNotSimplify / Ident guard

**Files:**
- Modify: `crates/starlink-ast/src/mappings/mod.rs` (Mapping trait)
- Modify: All mapping struct definitions (add field)
- Test: `crates/starlink-ast/tests/test_simplify_regression.rs`

- [ ] **Step 1: Write test**

Add to `crates/starlink-ast/tests/test_simplify_regression.rs`:

```rust
#[test]
fn do_not_simplify_prevents_merge() {
    use starlink_ast::mappings::{Mapping, ZoomMap, CmpMap, Composition};

    let mut z1 = ZoomMap::new(2, 3.0).unwrap();
    z1.set_do_not_simplify(true);
    let z2 = ZoomMap::new(2, 2.0).unwrap();

    // try_merge should return None when do_not_simplify is set.
    let merged = z1.try_merge(&z2, true, false, false);
    assert!(merged.is_none());

    // simplified() should return self unchanged.
    let s = z1.simplified().unwrap();
    assert!(s.as_any().is::<ZoomMap>());
}
```

- [ ] **Step 2: Design the implementation**

Add a `do_not_simplify: bool` field to a base struct or add it to the `Mapping` trait with a default implementation. The cleanest approach: add default methods to the trait:

```rust
    fn do_not_simplify(&self) -> bool { false }
    fn set_do_not_simplify(&mut self, _flag: bool) {}
```

Then override on each concrete type that stores the flag. Add the field to each mapping struct. The merge_loop in simplify.rs checks this flag before attempting merges.

- [ ] **Step 3: Implement**

Add the flag and accessor to each mapping struct. In `simplify.rs` `merge_loop`, before attempting a merge, check:

```rust
if list[i].0.do_not_simplify() || list[i+1].0.do_not_simplify() {
    i += 1;
    continue;
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/
git commit -m "Add DoNotSimplify guard to Mapping trait

When set, prevents simplified() from reducing and try_merge from
consuming the mapping. Protects user-tagged mappings from optimization."
```

---

## Task 33: map_split on Mapping trait

**Files:**
- Modify: `crates/starlink-ast/src/mappings/mod.rs` (add trait method)
- Modify: `crates/starlink-ast/src/mappings/{cmpmap,perm,winmap,zoom,shift,unit,matrix}.rs`
- Test: `crates/starlink-ast/tests/test_simplify_regression.rs`

- [ ] **Step 1: Write test**

```rust
#[test]
fn map_split_parallel_cmpmap() {
    use starlink_ast::mappings::{CmpMap, Composition, Mapping, ShiftMap, ZoomMap};

    let z = ZoomMap::new(2, 3.0).unwrap();
    let s = ShiftMap::new(&[1.0]);
    let par = CmpMap::new(Box::new(z), Box::new(s), Composition::Parallel).unwrap();

    // Split requesting outputs 0..2 (the ZoomMap portion).
    let (split, _rest) = par.map_split(0, 2).unwrap();
    assert_eq!(split.nin(), 2);
    assert_eq!(split.nout(), 2);
}
```

- [ ] **Step 2: Add trait method**

In `crates/starlink-ast/src/mappings/mod.rs`, add to the `Mapping` trait:

```rust
    /// Split this mapping to extract the sub-mapping producing a given
    /// range of output axes.
    ///
    /// Returns `(sub_mapping, remaining_mapping)` or `None` if inseparable.
    /// `first_out` is 0-based, `nout_wanted` is the count.
    fn map_split(&self, _first_out: usize, _nout_wanted: usize) -> Option<(DynMapping, DynMapping)> {
        None
    }
```

- [ ] **Step 3: Implement for key types**

Implement `map_split` for CmpMap (parallel decomposition), PermMap, WinMap, ZoomMap, ShiftMap, UnitMap, and diagonal MatrixMap. Each implementation extracts the sub-range of axes.

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/mappings/
git commit -m "Implement map_split on Mapping trait

Extracts sub-mappings for a given output axis range. Implemented for
CmpMap (parallel), PermMap, WinMap, ZoomMap, ShiftMap, UnitMap, and
diagonal MatrixMap. Used by CmpMap simplification, Region, FrameSet."
```

---

## Task 34: WcsMap NatLat/NatLon attributes

**Files:**
- Modify: `crates/starlink-ast/src/mappings/wcsmap.rs`
- Test: `crates/starlink-ast/tests/test_wcsmap.rs`

- [ ] **Step 1: Write test**

```rust
#[test]
fn wcsmap_natlat_natlon() {
    use starlink_ast::mappings::{WcsMap, WcsProjection};

    let wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    // TAN projection: native latitude of fiducial = 90° = π/2
    let nat_lat = wcs.nat_lat();
    assert!((nat_lat - std::f64::consts::FRAC_PI_2).abs() < 1e-10);
    // Native longitude of fiducial = 0 for zenithal projections.
    let nat_lon = wcs.nat_lon();
    assert!(nat_lon.abs() < 1e-10);
}
```

- [ ] **Step 2: Implement accessors**

Add `nat_lat()` and `nat_lon()` methods to `WcsMap` that query the underlying `PrjParams` for the native coordinates of the fiducial point.

- [ ] **Step 3: Run tests**

Run: `cargo test -p starlink-ast --test test_wcsmap wcsmap_natlat`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/starlink-ast/src/mappings/wcsmap.rs crates/starlink-ast/tests/test_wcsmap.rs
git commit -m "WcsMap: add NatLat/NatLon attribute accessors

Needed by FitsChan for FITS-WCS header generation."
```

---

## Remaining Tasks (summaries for subsequent phases)

The following tasks follow the same TDD pattern but are described at higher level since they depend on the infrastructure built in Tasks 1–34:

### Task 35: SphMap 3-mapping pattern (§3.4)
Detect `(inv-SphMap, diagonal-scaling, fwd-SphMap)` in simplify.rs merge loop, replace with WinMap.

### Task 36: WinMap swap past MatrixMap (§3.9)
Implement WinMat swap: absorb WinMap per-axis scale into MatrixMap columns.

### Task 37: WinMap swap past PermMap (§3.9)
Implement WinPerm swap: reorder WinMap terms per the permutation.

### Task 38: WinMap + parallel CmpMap distribution (§3.10)
Split WinMap by dimension groups, distribute into parallel branches.

### Task 39: WcsMap PermMap swapping (§3.11)
Swap WcsMap past adjacent PermMaps when the permutation brings lon/lat to expected positions.

### Task 40: LutMap linear→WinMap replacement (§3.13)
Detect linear LUTs in `simplified()`, replace with WinMap.

### Task 41: CmpMap recursion guard (§4.2)
Thread-local fingerprint stack to prevent infinite simplification recursion.

### Task 42: CmpMap PatternCheck/freeze (§4.3)
Detect oscillating simplification, freeze responsible mapping.

### Task 43: CmpMap relax equal-length constraint (§4.4)
Dimension-matching for series-in-parallel restructuring.

### Task 44: CmpMap PermMap constant transformation (§4.5)
Transform PermMap constants through CmpMap components during swap.

### Task 45: PolyMap linear decomposition (§4.9)
Decompose constant+linear PolyMap into ShiftMap + MatrixMap.

### Task 46: ChebyMap try_merge (§4.15)
Delegate to PolyMap's merge logic (share consolidation + cancellation).

### Task 47: MatrixMap swap-and-seek (§4.13)
Search past intermediate mappings for a merge target (up to N hops), swap toward it using WinMap swap operations from Tasks 36-37.

### Task 48: Adaptive numeric_rate (§5.2)
Port C's 9-point algorithm from mapping.c:8859-9152.

### Task 49: MatrixMap form-based fast paths (§5.4)
Add form field (Full/Diagonal/Unit), specialize transform paths.

---

## Verification Checkpoint

After Tasks 1–34 are complete:

- [ ] Run full test suite: `cargo test -p starlink-ast`
- [ ] Run clippy: `cargo clippy --workspace -- -D warnings`
- [ ] Run simplify regression: `cargo test -p starlink-ast --test test_simplify_regression`
- [ ] Run wcsconv tests: `cargo test -p starlink-ast --test test_wcsconv`
- [ ] If submodule available: `cargo test -p ast-oracle-tests`
