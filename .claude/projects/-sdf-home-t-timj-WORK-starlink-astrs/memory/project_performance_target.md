---
name: Performance target
description: Must be within 10% of C before Rust port is accepted — current status and optimization plan
type: project
---

The Rust port must achieve performance within 10% of the C library before it will be accepted as a replacement. Currently:

- **Forward 100K**: 336ns/pt (C: 363ns/pt) — **8% faster than C** ✓
- **Inverse 100K**: 2.0µs/pt (C: 1.1µs/pt) — **1.7x slower** ✗

**Why:** Acceptance criterion for replacing the C library in production pipelines (SCUBA-2, LSST).

**How to apply:** When the inverse gap needs closing, the optimization plan is:
1. Switch NR inner loop to flat `&[f64]` slices (avoid ndarray/PointSet overhead) — matches prototype approach that was only 10% slower than C
2. Remove per-point convergence check branching (process all points per iteration, check after)
3. Consider BLAS/Accelerate for bulk matrix operations in the polynomial evaluation
4. Profile to identify exact hotspots before guessing

The forward transform is already meeting the target. The inverse gap is in the NR polynomial evaluation loop overhead vs C's tighter inner loop.
