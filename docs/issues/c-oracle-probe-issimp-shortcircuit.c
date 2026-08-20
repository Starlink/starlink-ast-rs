#include "ast.h"
#include <stdio.h>
#include <string.h>
static FILE *src;
static char linebuf[ 4096 ];
static int ntag( AstObject *o ) {
   char *t = astToString( o ); int n = 0;
   for( const char *p = t; ( p = strstr( p, "IsSimp" ) ); p += 6 ) n++;
   astFree( t ); return n;
}
int main( int argc, char **argv ) {
   astBegin;
   src = fopen( argv[ 1 ], "r" );
   AstFitsChan *fc = astFitsChan( NULL, NULL, "Encoding=NATIVE" );
   while( fgets( linebuf, sizeof linebuf, src ) ) {
      size_t n = strlen( linebuf );
      while( n && ( linebuf[n-1] == '\n' || linebuf[n-1] == '\r' ) ) linebuf[--n] = 0;
      if( n ) astPutFits( fc, linebuf, 0 );
   }
   fclose( src ); astClear( fc, "Card" );
   AstFrameSet *fs = (AstFrameSet *) astRead( fc );

   AstMapping *map = astGetMapping( fs, AST__BASE, AST__CURRENT );
   printf( "base->current: class=%s IsSimple=%d tags=%d\n",
           astGetC( map, "Class" ), astGetI( map, "IsSimple" ), ntag( (AstObject *) map ) );

   AstMapping *s1 = astSimplify( map );
   printf( "after simplify: same-pointer=%d IsSimple=%d tags=%d\n",
           s1 == map, astGetI( s1, "IsSimple" ), ntag( (AstObject *) s1 ) );

   /* Now the same tree with the root's record absent: rebuild from the
      components so the nominate loop is forced to run. */
   if( !astIsACmpMap( map ) ) { printf( "  (root is not a CmpMap; no fresh-tree test)\n" ); astEnd; return 0; }
   AstMapping *a = NULL, *b = NULL; int series = 0, inva = 0, invb = 0;
   astDecompose( map, &a, &b, &series, &inva, &invb );
   if( !a || !b ) { printf( "  (decompose gave no pair)\n" ); astEnd; return 0; }
   AstMapping *fresh = (AstMapping *) astCmpMap( a, b, series, "" );
   printf( "fresh tree:     IsSimple=%d tags=%d\n",
           astGetI( fresh, "IsSimple" ), ntag( (AstObject *) fresh ) );
   AstMapping *s2 = astSimplify( fresh );
   printf( "after simplify: same-pointer=%d IsSimple=%d tags=%d\n",
           s2 == fresh, astGetI( s2, "IsSimple" ), ntag( (AstObject *) s2 ) );
   astEnd;
   return 0;
}
