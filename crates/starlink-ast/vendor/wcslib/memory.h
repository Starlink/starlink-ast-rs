#ifndef MEMORY_INCLUDED
#define MEMORY_INCLUDED
/*
 * Minimal stub for standalone wcslib compilation within starlink-ast.
 *
 * proj.c calls astSizeOf(ptr) to find the allocated length of the
 * dynamic p/p2 arrays in prjprm. In the full AST library this is a
 * macro expanding to astSizeOf_(ptr, &status). Here we provide a
 * declaration that satisfies the compiler; the actual implementation
 * will be linked from starlink-ast's Rust/C infrastructure at a later step.
 */
#include <stddef.h>

size_t astSizeOf_( const void *, int * );

#ifndef STATUS_PTR
static int ast_wcslib_status_ = 0;
#define STATUS_PTR &ast_wcslib_status_
#endif

#ifndef astERROR_INVOKE
#define astERROR_INVOKE(x) (x)
#endif

#define astSizeOf(ptr) astERROR_INVOKE(astSizeOf_(ptr, STATUS_PTR))

#endif
