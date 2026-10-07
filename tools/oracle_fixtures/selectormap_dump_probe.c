/* selectormap_dump_probe -- write C's dump of a SelectorMap of two Circles
 * in one Frame. C omits the Frame from the second Region (selectormap.c
 * Dump clears RegionFS), since the first Region's Frame is the Frame of
 * them all.
 *
 *   ./selectormap_dump_probe > ../../crates/ast-core/tests/selectormap_oracle/two_regions.ast
 */
#include <stdio.h>
#include "ast.h"
static void line( const char *l ) { printf( "%s\n", l ); }
int main( void ) {
   int status = 0; astWatch( &status );
   double c1[] = {0,0}, c2[] = {5,5}, r[] = {1};
   AstFrame *f = astFrame( 2, " " );
   AstRegion *regs[2];
   regs[0] = (AstRegion *) astCircle( f, 1, c1, r, NULL, " " );
   regs[1] = (AstRegion *) astCircle( f, 1, c2, r, NULL, " " );
   AstChannel *ch = astChannel( NULL, line, " " );
   astWrite( ch, astSelectorMap( 2, (void **) regs, AST__BAD, " " ) );
   return 0;
}
