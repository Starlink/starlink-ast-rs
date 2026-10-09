/* region_moc_probe -- capture the Moc class core (moc.c): Mocs built cell by
 * cell with astAddCell (OR, XOR and AND, at several orders), one whose
 * MaxOrder needs 8 byte values; for each, its dump and that dump read back,
 * its MocLength, MocType, MocArea and normalised cells (astGetCell), the
 * membership of a grid of sky positions (plain, negated, and with System set
 * to Galactic through the Moc), its boundary mesh (astGetRegionMesh) and
 * bounding box; astTestCell; the overlap of a Moc with a Circle each way
 * round; the mask astRegPins gives for boundary points supplied out of
 * Dec order; and the error status of an invalid cell and of setting
 * MaxOrder twice. It needs moc.c as fixed upstream in 2b631e08 and
 * c95aa801: without them astGetRegionBounds does not return for the empty
 * Moc, the RegPins mask flags the wrong points, an empty Moc's Transform
 * indexes out of bounds, and the mesh depends on the platform's qsort.
 *
 * Prints Rust source for crates/ast-core/tests/region_moc_oracle/cases.rs:
 *
 *   ./region_moc_probe > ../../crates/ast-core/tests/region_moc_oracle/cases.rs
 */
#include <stdio.h>
#include <string.h>
#include "ast.h"

extern int astRegPins_( AstRegion *, AstPointSet *, AstRegion *, int **, int * );
extern AstPointSet *astPointSet_( int, int, const char *, int *, ... );
extern double **astGetPoints_( AstPointSet *, int * );

static char dump[ 400000 ];
static char reload_buf[ 400000 ];
static const char *next_line;

static void sink( const char *line ) {
   strcat( dump, line );
   strcat( dump, "\n" );
}

static const char *source( void ) {
   static char line[ 4000 ];
   const char *nl;
   size_t n;
   if( !next_line || !*next_line ) return NULL;
   nl = strchr( next_line, '\n' );
   n = nl ? (size_t)( nl - next_line ) : strlen( next_line );
   memcpy( line, next_line, n );
   line[ n ] = 0;
   next_line = nl ? nl + 1 : next_line + n;
   return line;
}

static void str( const char *s ) {
   putchar( '"' );
   for( ; *s; s++ ) {
      if( *s == '\n' ) {
         printf( "\\n" );
      } else if( *s == '\t' ) {
         printf( "\\t" );
      } else {
         if( *s == '"' || *s == '\\' ) putchar( '\\' );
         putchar( *s );
      }
   }
   putchar( '"' );
}

static const char *dumped( AstObject *obj ) {
   AstChannel *ch;
   dump[ 0 ] = 0;
   ch = astChannel( NULL, sink, " " );
   astWrite( ch, obj );
   ch = astAnnul( ch );
   return dump;
}

static const char *reloaded( const char *text ) {
   AstChannel *ch;
   AstObject *obj;
   strcpy( reload_buf, text );
   next_line = reload_buf;
   ch = astChannel( source, NULL, " " );
   obj = astRead( ch );
   ch = astAnnul( ch );
   return dumped( obj );
}

/* A double as a Rust f64 literal. */
static void num( double v ) {
   char buf[ 40 ];
   sprintf( buf, "%.17g", v );
   printf( "%s%s", buf, strpbrk( buf, ".en" ) ? "" : ".0" );
}

#define NGRID 72
static double gx[ NGRID ], gy[ NGRID ];

static void grid( void ) {
   int i, j, k = 0;
   for( j = 0; j < 6; j++ ) {
      for( i = 0; i < 12; i++ ) {
         gx[ k ] = ( i + 0.37 )*( 2*AST__DPI/12.0 );
         gy[ k ] = -1.3 + j*0.52;
         k++;
      }
   }
}

/* The grid transformed, as a list of `[x out, y out]`. */
static void transformed( AstObject *reg ) {
   double xo[ NGRID ], yo[ NGRID ];
   int i;
   astTran2( reg, NGRID, gx, gy, 1, xo, yo );
   printf( "&[" );
   for( i = 0; i < NGRID; i++ ) {
      printf( "[" );
      num( xo[ i ] );
      printf( ", " );
      num( yo[ i ] );
      printf( "]%s", i < NGRID - 1 ? ", " : "" );
   }
   printf( "]" );
}

/* One Moc: `(name, dump, dump read back, MocLength, MocType, MocArea,
   cells, grid transformed, grid transformed by the negated Moc, mesh,
   bounds)`. */
static void emit( const char *name, AstMoc *moc ) {
   static char first[ 400000 ];
   int i, len, order, npnt;
   int64_t npix;
   double lbnd[ 2 ], ubnd[ 2 ];
   double *mesh;
   AstMoc *neg;

   printf( "    (\"%s\",\n        ", name );
   strcpy( first, dumped( (AstObject *) moc ) );
   str( first );
   printf( ",\n        " );
   str( reloaded( first ) );
   len = astGetI( moc, "MocLength" );
   printf( ",\n        %d, %d, ", len, astGetI( moc, "MocType" ) );
   num( astGetD( moc, "MocArea" ) );
   printf( ",\n        &[" );
   for( i = 0; i < len; i++ ) {
      astGetCell( moc, i, &order, &npix );
      printf( "(%d, %lld)%s", order, (long long) npix, i < len - 1 ? ", " : "" );
   }
   printf( "],\n        " );
   transformed( (AstObject *) moc );
   printf( ",\n        " );
   neg = astCopy( moc );
   astNegate( neg );
   transformed( (AstObject *) neg );
   printf( ",\n        &[" );
   astGetRegionMesh( moc, 1, 0, 2, &npnt, NULL );
   mesh = astMalloc( sizeof( double )*2*npnt );
   astGetRegionMesh( moc, 1, npnt, 2, &npnt, mesh );
   for( i = 0; i < npnt; i++ ) {
      printf( "[" );
      num( mesh[ i ] );
      printf( ", " );
      num( mesh[ npnt + i ] );
      printf( "]%s", i < npnt - 1 ? ", " : "" );
   }
   mesh = astFree( mesh );
   astGetRegionBounds( moc, lbnd, ubnd );
   printf( "],\n        [" );
   num( lbnd[ 0 ] );
   printf( ", " );
   num( lbnd[ 1 ] );
   printf( ", " );
   num( ubnd[ 0 ] );
   printf( ", " );
   num( ubnd[ 1 ] );
   printf( "]),\n" );
}

int main( void ) {
   int status = 0;
   AstMoc *low, *high, *gal, *empty;
   AstCircle *circle;
   AstPointSet *pins;
   double **pptr, *mesh;
   double centre[ 2 ], radius;
   int *mask, result, npnt, i;

   astWatch( &status );
   grid();
   printf( "// Generated by tools/oracle_fixtures/region_moc_probe.c. Do not edit.\n\n" );

   low = astMoc( " " );
   astAddCell( low, AST__OR, 2, 37 );
   astAddCell( low, AST__OR, 1, 3 );
   astAddCell( low, AST__OR, 2, 100 );
   astAddCell( low, AST__XOR, 1, 25 );
   astAddCell( low, AST__XOR, 2, 101 );
   astAddCell( low, AST__AND, 2, 13 );
   astAddCell( low, AST__OR, 0, 7 );
   astAddCell( low, AST__AND, 2, 30 );

   high = astMoc( " " );
   astAddCell( high, AST__OR, 15, 1234567890 );
   astAddCell( high, AST__OR, 10, 5 );
   astAddCell( high, AST__OR, 3, 700 );

   gal = astCopy( low );
   astSet( gal, "System=Galactic" );

   empty = astMoc( " " );

   printf( "/// `(case, dump, dump read back, MocLength, MocType, MocArea, cells as\n" );
   printf( "/// (order, npix), a grid of positions transformed, the same by the negated\n" );
   printf( "/// Moc, boundary mesh, bounds [lra, ldec, ura, udec])`.\n" );
   printf( "pub type Case = (&'static str, &'static str, &'static str, i32, i32, f64,\n" );
   printf( "    &'static [(i32, i64)], &'static [[f64; 2]], &'static [[f64; 2]],\n" );
   printf( "    &'static [[f64; 2]], [f64; 4]);\n" );
   printf( "pub const CASES: &[Case] = &[\n" );
   emit( "low", low );
   emit( "high", high );
   emit( "galactic", gal );
   emit( "empty", empty );
   printf( "];\n\n" );

   printf( "/// The grid of positions, as `[ra, dec]`.\n" );
   printf( "pub const GRID: &[[f64; 2]] = &[" );
   for( i = 0; i < NGRID; i++ ) {
      printf( "[" );
      num( gx[ i ] );
      printf( ", " );
      num( gy[ i ] );
      printf( "]%s", i < NGRID - 1 ? ", " : "" );
   }
   printf( "];\n\n" );

   printf( "/// astTestCell( low, order, npix, parent ) for each `(order, npix, parent)`.\n" );
   printf( "pub const TEST_CELLS: &[(i32, i64, bool, bool)] = &[" );
   {
      int t[][3] = { {2,37,0}, {1,3,0}, {2,12,0}, {2,12,1}, {2,13,1}, {0,7,1},
                     {0,7,0}, {1,9,0}, {2,101,1}, {2,30,1} };
      for( i = 0; i < 10; i++ ) {
         printf( "(%d, %d, %s, %s)%s", t[i][0], t[i][1], t[i][2] ? "true" : "false",
                 astTestCell( low, t[i][0], t[i][1], t[i][2] ) ? "true" : "false",
                 i < 9 ? ", " : "" );
      }
   }
   printf( "];\n\n" );

   centre[ 0 ] = 1.2;
   centre[ 1 ] = 0.3;
   radius = 0.6;
   circle = astCircle( astSkyFrame( "System=ICRS" ), 1, centre, &radius, NULL, " " );
   printf( "/// astOverlap of the low Moc with an ICRS Circle, each way round.\n" );
   printf( "pub const OVERLAP_MOC_CIRCLE: i32 = %d;\n", astOverlap( low, circle ) );
   printf( "pub const OVERLAP_CIRCLE_MOC: i32 = %d;\n\n", astOverlap( circle, low ) );

   /* Pins: the low Moc's mesh points in reverse order, then one point that
      is not on the boundary. */
   astGetRegionMesh( low, 1, 0, 2, &npnt, NULL );
   mesh = astMalloc( sizeof( double )*2*npnt );
   astGetRegionMesh( low, 1, npnt, 2, &npnt, mesh );
   pins = astPointSet_( npnt + 1, 2, " ", &status );
   pptr = astGetPoints_( pins, &status );
   for( i = 0; i < npnt; i++ ) {
      pptr[ 0 ][ i ] = mesh[ npnt - 1 - i ];
      pptr[ 1 ][ i ] = mesh[ 2*npnt - 1 - i ];
   }
   pptr[ 0 ][ npnt ] = 3.0;
   pptr[ 1 ][ npnt ] = -1.4;
   result = astRegPins_( astMakePointer( low ), pins, NULL, &mask, &status );
   printf( "/// astRegPins of the low Moc's mesh in reverse order, then one point off\n" );
   printf( "/// its boundary: the result, and the mask.\n" );
   printf( "pub const PINS_RESULT: bool = %s;\n", result ? "true" : "false" );
   printf( "pub const PINS_MASK: &[bool] = &[" );
   for( i = 0; i <= npnt; i++ ) printf( "%s%s", mask[ i ] ? "true" : "false", i < npnt ? ", " : "" );
   printf( "];\n\n" );
   mask = astFree( mask );
   mesh = astFree( mesh );

   astAddCell( low, AST__OR, 3, 5 );
   printf( "/// The status of adding a cell at an order above MaxOrder.\n" );
   printf( "pub const CELL_ORDER: i32 = %d;\n", status );
   astClearStatus;
   astSetI( low, "MaxOrder", 3 );
   printf( "/// The status of setting MaxOrder once it is set.\n" );
   printf( "pub const MAX_ORDER_SET: i32 = %d;\n", status );
   astClearStatus;

   return 0;
}
