#include <stddef.h>

/*
 * Stub implementation of astSizeOf_ for standalone wcslib compilation.
 *
 * In the full AST library, astSizeOf_ returns the allocated size of a
 * memory block tracked by AST's memory subsystem. Here we return 0
 * which causes the ZPN projection initialiser to treat the p array as
 * empty — this is safe because:
 *
 * 1. During bootstrap, no code actually calls the ZPN projection.
 * 2. Once ast-core provides its real memory infrastructure, this stub
 *    will be replaced by the proper implementation.
 */
size_t astSizeOf_( const void *ptr, int *status ) {
    (void)ptr;
    (void)status;
    return 0;
}
