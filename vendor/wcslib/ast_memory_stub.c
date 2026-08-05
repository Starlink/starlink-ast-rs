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
 * (WCSLIB_MXPAR + 1)-length double buffer (PRJ_PARAM_SLOTS in wcslib.rs),
 * matching the reference library, which allocates mxpar + 1 doubles per axis
 * (wcsmap.c:5862) so that index mxpar itself is addressable.  Reporting that
 * size lets astZPNset locate the coefficient degree exactly as the reference
 * library does, and keeps its scan inside the buffer.
 */
size_t astSizeOf_( const void *ptr, int *status ) {
    (void)ptr;
    (void)status;
    return ( WCSLIB_MXPAR + 1 ) * sizeof( double );
}
