---
name: Follow C algorithmic approaches
description: Port should study and follow C library algorithms, not reinvent from first principles
type: feedback
---

The Rust port should examine and follow the algorithmic approaches used in the C library (reference/starlink-ast/src/) rather than inventing new approaches from first principles. The C library has been optimised over 20+ years and its algorithms are well-tested.

**Why:** The PolyMap polynomial evaluation was implemented using naive `powi()` per term instead of the C library's power-table approach — resulting in a 4x performance gap that the C's algorithm would have avoided from the start. The prototype (starlink-astrs-proto) followed the C approach and was faster than C on forward transforms.

**How to apply:** When implementing any computation-heavy class (Mapping transforms, SlaMap conversions, etc.), always read the C implementation first to understand the algorithmic approach. Use it as the guide for the Rust implementation, adapting idiomatically where Rust can do better (e.g., ownership, safety) but preserving the core data structures and evaluation strategy.
