#include <stddef.h>

#include "proj.h"

/*
 * astSizeOf_ for the ast-core FFI build of wcslib.
 *
 * In the full AST library astSizeOf_ returns the allocated byte size of a
 * memory block tracked by AST's memory subsystem.  Its only caller in this
 * vendored subset is astZPNset (proj.c), which computes
 *
 *     plen = astSizeOf( prj->p )/sizeof( double );
 *
 * and scans prj->p[plen-1 .. 0] for the highest nonzero polynomial
 * coefficient.  ast-core always backs prj->p and prj->p2 with a
 * WCSLIB_MXPAR-length double buffer (see PrjParams in wcslib.rs), so the
 * true allocation size of every block passed here is
 * WCSLIB_MXPAR * sizeof( double ).  Reporting that lets astZPNset locate
 * the coefficient degree exactly as the reference library does.
 */
size_t astSizeOf_( const void *ptr, int *status ) {
    (void)ptr;
    (void)status;
    return WCSLIB_MXPAR * sizeof( double );
}
