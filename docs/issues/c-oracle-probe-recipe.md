# Building and running a C-oracle probe

When a Rust behaviour is questioned, the C library is the source of truth.
The fastest way to settle a question is a tiny standalone C program linked
against the repo's own `libast`, calling the same functions the Rust port
mirrors.
This document is the recipe, including the two traps that cost the most time
to rediscover: the stale conda `libast`, and calling protected (non-public)
AST functions.

## TL;DR recipe

```sh
cd /path/to/starlink-astrs
R=$PWD/reference/starlink-ast/build-ast     # cmake build output (see below)

cc probe.c grf_stubs.c -o /tmp/probe \
  -I "$R" -L "$R" \
  -Wl,-rpath,"$R" -Wl,--disable-new-dtags \
  -last -last_pal -last_cminpack -lyaml -lm

LD_LIBRARY_PATH= /tmp/probe                  # note the empty LD_LIBRARY_PATH
```

- `-I "$R" -L "$R"` — use the repo build's `ast.h` and `libast.so`.
- `-Wl,-rpath,"$R" -Wl,--disable-new-dtags` — bake the repo build's path in
  as a **DT_RPATH** (not DT_RUNPATH), so it wins over `LD_LIBRARY_PATH`.
- `LD_LIBRARY_PATH=` — clear it at run time as a belt-and-braces measure
  against the conda trap below.
- `-lyaml -lm` — `libast` pulls in yaml (serialisation) and libm.
- `-last_pal -last_cminpack` — separate shared libraries since AST 9.5.0, so
  the linker needs them named. Without them `libast.so` leaves `astPal*`,
  `astEra*` and the cminpack symbols undefined. They are AST's own forks, not
  the system pal/cminpack, so they must come from `$R` rather than conda.
- `grf_stubs.c` — satisfies the grf/grf3d/err plotting symbols the linker
  wants but a probe never calls (see "Satellite symbols" below).

## Trap 1 — the stale conda `libast`

The lsst-scipipe conda env ships its own AST:

```
$CONDA_PREFIX/lib/libast.a         (and libast.so.9.3.1, plus libast_* variants)
$CONDA_PREFIX/lib/libyaml*
```

That build is **AST 9.3.1** and is NOT the code under test.
If a probe silently links or loads it, you will be debugging the wrong
library and the results will look "correct" for reasons that have nothing to
do with the repo.

The `conda activate` for this project puts `$CONDA_PREFIX/lib` on
`LD_LIBRARY_PATH`.
A DT_RUNPATH baked into the binary (the default with modern binutils) is
consulted **after** `LD_LIBRARY_PATH`, so the conda copy wins even when you
`-L` the repo build.
Two independent defences, use both:

1. `-Wl,-rpath,"$R" -Wl,--disable-new-dtags` forces a **DT_RPATH** entry,
   which the loader consults *before* `LD_LIBRARY_PATH`.
2. Run with `LD_LIBRARY_PATH=` cleared.

Verify what actually loaded:

```sh
LD_LIBRARY_PATH= ldd /tmp/probe | grep -i ast
# must point at reference/starlink-ast/build-ast, NOT $CONDA_PREFIX
```

(Same trap, same fix as the recorded `reference_oracle_conda_rpath_trap`
memory — it bites the oracle-test crates too, not just ad-hoc probes.)

## Building the reference `libast`

Build with **cmake**, never autotools (recorded in the
`reference_oracle_build_cmake` memory):

```sh
git submodule update --init reference/starlink-ast
cd reference/starlink-ast
cmake -S . -B build-ast -DCMAKE_C_FLAGS=-ffp-contract=off
cmake --build build-ast -j
```

This produces `build-ast/ast.h` and `build-ast/libast.so*`.
There is also a `build/` tree from an earlier configuration; either works,
but keep `$R` pointing at whichever you actually built last.

### Trap 1a — floating-point contraction in the oracle

Without `-ffp-contract=off`, clang on Apple arm64 fuses expressions such as
`value += a * b * c` into a single multiply-add, which rounds once instead
of twice.
The answer then differs from the arithmetic the C source writes by an ULP or
so, and from a Linux x86-64 build, which has no fused multiply-add by
default.
The port computes what the source writes, as the vendored C does (it is
built with the same flag), so an oracle built with contraction reports
spurious last-bit differences.
Check a build with `otool -tv libast.dylib | grep -c fmadd`, which is zero
when contraction is off.
A table that passes only against a contracted build was captured with the
wrong oracle; regenerate it.

### Trap 1b — a `libast` older than the pinned source

The two build trees do not rebuild themselves, so one of them is usually
behind `reference/starlink-ast/src/`.
A probe linked against the older one answers the question for whatever AST
was current when that tree was last built, and says nothing about the pin.
It fails silently and convincingly: the probe runs, `astOK` is 1, and the
answer is simply the old behaviour.
Check before trusting a result:

```sh
ls -l --time-style=+%F "$R"/libast.so.9.* reference/starlink-ast/src/<class>.c
# the library must be newer than the source file the question is about
```

If it is not, rebuild that tree (`cmake --build "$R" -j`) or point `$R` at the
other one.

## Trap 2 — calling protected (internal) AST functions

Public functions declared in `ast.h` (e.g. `astOverlap`, `astCircle`,
`astCmpRegion`, `astNegate`) can be called directly — no ceremony.
Many interesting predicates, however, are **protected**: they are the
`astFoo_` C-level entry points behind macros that are only visible when a
class is compiling itself. `astGetBounded`, `astGetNegated`, and most
`astGet<Attr>`/`astTest<Attr>` on Region internals are in this group and are
**not** in the public `ast.h`.

Two things are needed to call them from a probe:

### 2a. Declare the raw `astFoo_` entry point yourself

The symbol exists in `libast.so` (verify with `nm -D`), it just is not
declared in the public header.
Declare it and call it directly with an explicit status pointer:

```c
extern int astGetBounded_(void *region, int *status);
extern int astGetNegated_(void *region, int *status);
```

The `_`-suffixed form takes the object pointer plus an `int *status` as its
final argument. Do NOT try to `#include` the internal `region.h` /
`nullregion.h` — they redefine the struct types already in the public
`ast.h` and you get a cascade of redefinition errors, missing internal
headers (`sae_par.h`, `ast_err.h`, `config.h`), `int64_t` breakage, and a
`STATUS_PTR` clash (`STATUS_PTR` is a macro `astGetStatusPtr`, not a local).
Declaring the one `extern` you need sidesteps all of it.

### 2b. Convert the public handle ID to a real pointer

This is the subtle one, and the cause of the persistent segfaults.
The public constructors (`astFrame`, `astNullRegion`, `astCircle`,
`astCmpRegion`, …) do **not** return C pointers. They return small integer
**handle IDs** (you will see values like `0x5f101`, `0x5f002` — clearly not
addresses).
Passing a handle ID straight into a protected `astFoo_` that expects a real
pointer dereferences garbage and segfaults.

Convert with the public `astMakePointer`:

```c
static int bounded(void *id) {
    int s = 0;
    return astGetBounded_(astMakePointer(id), &s);
}
```

`astMakePointer` is public (it is in `ast.h`) and maps a handle ID to the
underlying object pointer. Wrap every protected call in a helper that does
the conversion.

### Full worked probe (the boundedness question)

```c
#include <stdio.h>
#include "ast.h"
extern int astGetBounded_(void *region, int *status);
static int B(void *id){ int s=0; return astGetBounded_(astMakePointer(id), &s); }

int main(void) {
    int status = 0;
    astWatch(&status);                       /* route errors to our status  */
    AstFrame *frm = astFrame(2, " ");
    void *n1 = astNullRegion(frm, NULL, " ");
    void *nn = astNullRegion(frm, NULL, " "); astNegate((AstRegion*)nn);
    double cen[2]={0,0}, p[2]={3,0};
    void *c  = astCircle(frm, 1, cen, p, NULL, " ");
    printf("null(unneg) bounded = %d\n", B(n1));   /* 1 */
    printf("null(neg)   bounded = %d\n", B(nn));   /* 0 */
    printf("circle      bounded = %d\n", B(c));    /* 1 */
    printf("null AND null       = %d\n",
           B(astCmpRegion((AstRegion*)n1,(AstRegion*)n1,AST__AND," ")));
    printf("astOK = %d\n", astOK);
    return 0;
}
```

## Worked probe — the clear-on-set convention

A second worked example, this time using only the public string interface.
`IsSimple` is reachable through it (`mapping.c:1564` handles the name in
`GetAttrib`), as are SphMap's `UnitRadius` (`sphmap.c:448`) and `PolarLong`
(`:457`), so no `astCLASS` ceremony and no `astMakePointer` are needed.

Two things this probe has to get right:

- `astSimplify` returns a clone when nothing simplifies, so the original
  pointer must be annulled before the set.
  SphMap's setter is `astMAKE_SET1`, which refuses to change an object with
  more than one reference (`object.h:1218`), and the resulting `AST__IMMUT`
  error then makes every later `astGetI` return the attribute's bad value —
  `0`, which is indistinguishable from a cleared record.
- The negative control matters more than the positive cases.
  `Invert` is not one of the attributes C covers, and `mapping.h:424-437`
  requires inverting and inverting back to leave the Mapping reporting what it
  reported before, so that case must print `1` where the others print `0`.
  Without it, an error state or a build that never sets the flag reads as a
  pass.

```c
#include <stdio.h>
#include "ast.h"

static AstMapping *simplified_sphmap(void) {
    AstSphMap *m = astSphMap("");
    AstMapping *s = astSimplify(m);
    astAnnul(m);
    return s;
}

int main(void) {
    int status = 0;
    astWatch(&status);

    AstMapping *s = simplified_sphmap();
    printf("after simplify:          IsSimple = %d\n", astGetI(s, "IsSimple"));
    astSetD(s, "PolarLong", 1.0);
    printf("after set PolarLong:     IsSimple = %d\n", astGetI(s, "IsSimple"));

    AstMapping *v = simplified_sphmap();
    astSetI(v, "UnitRadius", 1);
    printf("after set UnitRadius:    IsSimple = %d\n", astGetI(v, "IsSimple"));

    AstMapping *x = simplified_sphmap();
    astClear(x, "PolarLong");
    printf("after clear PolarLong:   IsSimple = %d\n", astGetI(x, "IsSimple"));

    AstMapping *y = simplified_sphmap();
    astSetI(y, "Invert", 1);
    astSetI(y, "Invert", 0);
    printf("after invert and back:   IsSimple = %d\n", astGetI(y, "IsSimple"));

    printf("astOK = %d\n", astOK);
    return 0;
}
```

Against a `libast` built from the current pin:

```
after simplify:          IsSimple = 1
after set PolarLong:     IsSimple = 0
after set UnitRadius:    IsSimple = 0
after clear PolarLong:   IsSimple = 0
after invert and back:   IsSimple = 1
astOK = 1
```

Against the stale `build-ast` tree the first two lines read `1` and `1`, which
is what trap 1b looks like from the outside.

## Gotchas that produce segfaults or wrong answers

- **`astCircle` needs a real second point.** Passing `NULL` for the `point`
  argument (rather than a valid `double[naxes]`) crashes inside construction.
  Use e.g. `cen={0,0}`, `p={3,0}` for a radius-3 circle.
- **Handle vs pointer** (2b above) — the single biggest time sink. If a
  protected call segfaults, check you ran it through `astMakePointer`.
- **`-Wincompatible-pointer-types`** on `AstNullRegion*`/`AstCircle*` →
  `AstRegion*` are only warnings; a probe compiled with `-Werror` will stop
  on them. Cast to `(AstRegion*)` or drop `-Werror`.
- Always print `astOK` at the end. A `0` means an AST error was raised
  mid-run and every result above it is suspect.

## Satellite symbols — `grf_stubs.c`

`libast` references the grf (2D plot), grf3d (3D plot), and `astPutErr_`
error-reporting symbols that a normal application supplies. A probe plots
nothing, so link a stub file that defines them as no-ops. Keep a copy of
`grf_stubs.c` next to your probe; it defines `astGAttr`, `astGLine`,
`astGText`, … the `astG3D*` trio, and `astPutErr_`. A working copy lives next
to this doc at
[`c-oracle-probe-grf_stubs.c`](c-oracle-probe-grf_stubs.c); if it ever needs
extending, every missing symbol from the linker's "undefined reference" list
is a one-line no-op returning `0`.

`astPal*`, `astEra*` and the cminpack symbols are a different problem and the
stubs do not cover them: they are real implementations in `libast_pal` and
`libast_cminpack`, which 9.5.0 builds as separate shared libraries, so they are
named on the link line rather than stubbed.

## macOS link line

On macOS the build is `libast.dylib`.
A probe linked against `reference/starlink-ast/build` needs the satellite libraries named on the link line, or it crashes on the first AST error:

```sh
R=$PWD/reference/starlink-ast/build
cc probe.c -o probe -I "$R" -L "$R" -L/opt/homebrew/lib -Wl,-rpath,"$R" \
  -last -last_pal -last_cminpack -last_err -last_grf3d -last_grf_5.6 -lyaml -lm
```

`-L/opt/homebrew/lib` is where Homebrew's libyaml lives; drop it if yours is elsewhere on the default path.

## When NOT to build a probe

If the C source already answers the question unambiguously, read it and stop.
For the boundedness fix, `region.c:3358` (base `GetBounded` = `!Negated`,
`1` inside a SkyFrame) plus `grep -c Bounded nullregion.c` returning `0` (no
override) was already definitive; the probe only *confirmed* it. Reach for a
probe when control flow depends on a runtime value you cannot read off the
source — e.g. the exact `astOverlap` code returned for a specific pair of
regions, which then selects a branch in `cmpregion.c::GetBounded`.
