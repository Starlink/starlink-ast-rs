# Analysis: sph_fwd_inv_unitradius_cancel fixture discrepancy

## Fixture files

- **Input:** `tests/fixtures/native/reference/simplify_fixtures/sph_fwd_inv_unitradius_cancel.map`
- **C reference:** `tests/fixtures/native/reference/simplify_fixtures/sph_fwd_inv_unitradius_cancel.simp`

## Input mapping (native format)

```
Begin CmpMap
    Nin = 3
 IsA Mapping
    InvB = 1
    MapA =
       Begin SphMap
          Nin = 3
          Nout = 2
       IsA Mapping
          UntRd = 1
       End SphMap
    MapB =
       Begin SphMap
          Nin = 3
          Nout = 2
          Invert = 1
       IsA Mapping
          UntRd = 1
       End SphMap
End CmpMap
```

## C reference simplification

```
Begin UnitMap
    Nin = 2
    IsSimp = 1
 IsA Mapping
End UnitMap
```

## Rust simplification

`UnitMap(Nin = 3)`

## Analysis

### Tracing the inversion flags

The CmpMap is a series composition with:
- `this->invert1 = 0` (InvA absent, defaults to 0)
- `this->invert2 = 1` (InvB = 1)
- CmpMap's own `Invert = 0` (absent, defaults to 0)

MapA is a SphMap with stored Invert = 0 (absent).
MapB is a SphMap with stored Invert = 1.

### C MapList decomposition (cmpmap.c lines 1148-1186)

For forward decomposition of the CmpMap (external `invert = 0`):

```c
invert1 = this->invert1;   // = 0
invert2 = this->invert2;   // = 1  (from InvB = 1)

if (invert) {               // invert = 0, so this block is skipped
    invert1 = !invert1;
    invert2 = !invert2;
}

// Series, not inverted: MapA first, MapB second:
astMapList(this->map1, series, invert1=0, ...)
astMapList(this->map2, series, invert2=1, ...)
```

Result: flat list is `[SphMap_A with apply-invert=0, SphMap_B with apply-invert=1]`

The `invert_list` values tell the simplification engine what Invert
attribute value to SET on each mapping before applying it.

### Effective directions

**MapA** (SphMap, native Nin=3 Nout=2, applied with invert_list=0):
- Set Invert=0 → GetNin()=3, GetNout()=2
- Transform forward: effective_fwd = true XOR Invert(0) = true
- **Native forward = Cartesian → Spherical (3→2)**

**MapB** (SphMap, native Nin=3 Nout=2, applied with invert_list=1):
- Set Invert=1 → GetNin()=2, GetNout()=3 (dimensions swap)
- Transform forward: effective_fwd = true XOR Invert(1) = false
- **Inverse direction = Spherical → Cartesian (2→3)**

### Effective pipeline

```
Input (3D Cartesian)
  → MapA: cart→sph (3→2)
  → MapB: sph→cart (2→3)
Output (3D Cartesian)
```

Total: 3→2→3 = **3D identity** (for unit-radius inputs, guaranteed
by UntRd=1 on both SphMaps).

### Dimension verification

- CmpMap.GetNin() = MapA.GetNin(with invert=0) = 3
- CmpMap.GetNout() = MapB.GetNout(with invert=1) = 3
- The CmpMap serialization confirms: `Nin = 3` (Nout absent = same as Nin = 3)

### Expected simplification

A 3→2→3 identity pipeline should simplify to **UnitMap(Nin = 3)**.

### Discrepancy

The C reference `.simp` file contains **UnitMap(Nin = 2)**. This is
dimensionally inconsistent:

- The input CmpMap has Nin=3 (both by serialized attribute and by
  component analysis)
- UnitMap(2) is a 2→2 mapping
- Replacing a 3→3 mapping with a 2→2 mapping changes the interface

### Possible explanations

1. **C simplification bug**: The SphMap `MapMerge` code (sphmap.c
   around line 760) computes the UnitMap dimension from the wrong
   quantity. The C code determines the result dimension from the
   SphMap's "nin" in some direction. If it uses the SphMap's nout=2
   (the spherical-coordinate dimension) instead of the effective
   input dimension of the series pair (3), it produces UnitMap(2).

2. **Fixture generation used a different CmpMap state**: If the C
   test harness set the CmpMap's own Invert flag before simplifying
   (making the pipeline 2→3→2 = sph→cart→sph = 2D identity), then
   UnitMap(2) would be correct. But the serialized CmpMap has no
   Invert flag (defaults to 0).

3. **Different interpretation of InvB + MapB.Invert interaction**:
   If the C code XORs `invert2` (from InvB=1) with MapB's stored
   Invert (=1) giving effective invert = 0 (cancellation), then
   MapB would be applied forward (cart→sph, 3→2). The pipeline
   would be 3→2→2 which is dimensionally impossible for a series
   composition. This interpretation seems unlikely.

4. **Alternative interpretation**: If `invert_list` values represent
   XOR with the stored Invert (rather than replacement), then:
   - MapB: stored=1 XOR applied=1 = 0 → use forward → cart→sph (3→2)
   - Pipeline: 3→2→2 — dimensionally invalid

   This would be a bug in the interpretation, not a valid state.

### Questions for C developers

1. What is `astGetNin(cmpmap)` when this CmpMap is loaded from the
   `.map` file above? Is it 3 or 2?

2. If you call `astSimplify(cmpmap)`, does it really produce
   UnitMap(2)? Or was this fixture generated incorrectly?

3. In the C MapList decomposition, are the `invert_list` values
   meant to REPLACE the mapping's stored Invert attribute, or XOR
   with it?

4. Is there a case where `InvB=1` on the CmpMap combined with
   `Invert=1` on the stored SphMap should cancel (XOR to 0) rather
   than compound? The MapList code at line 1155 (`invert2 =
   this->invert2`) suggests they do NOT interact with the stored
   Invert — but if there's additional logic elsewhere that resolves
   them, that would explain the different dimension.
