/* region_moc_add_probe -- capture Mocs built by astAddRegion and
 * astAddPixelMask<X> (moc.c): Circles, a Polygon, a negated Circle, a
 * Circle round the pole, a Galactic Circle and a Box in a CmpFrame with a
 * spectral axis, added with OR, XOR and AND; MinOrder at and above
 * MaxOrder, the latter also with a negated Circle; Mocs added to Mocs at lower, higher and unset MaxOrder, negated
 * and empty; a MaxOrder chosen from the size of a Circle; and pixel masks
 * of double, float and int arrays under each operator, with a bad value,
 * through a FrameSet with its sky axes in either order, with MaxOrder
 * chosen from the pixel size, and selecting nothing. For each, its MaxOrder,
 * MocLength, MocArea, an FNV-1a hash of its dump and, when short, the dump.
 * Also the error status of a Region with no sky axes, and of a Region and
 * a pixel mask added with MaxOrder 0. It needs moc.c as fixed upstream in
 * 28473801: without it a negated Region added with MinOrder above MaxOrder
 * covers the sky several times over, and a Region added with MaxOrder 0
 * reads out of bounds.
 *
 * Prints Rust source for crates/starlink-ast/tests/region_moc_add_oracle/cases.rs:
 *
 *   ./region_moc_add_probe > ../../crates/starlink-ast/tests/region_moc_add_oracle/cases.rs
 */
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "ast.h"

#define NX 20
#define NY 15

static char dump[ 4000000 ];

static void sink( const char *line ) {
   strcat( dump, line );
   strcat( dump, "\n" );
}

static const char *dumped( AstObject *obj ) {
   AstChannel *ch;
   dump[ 0 ] = 0;
   ch = astChannel( NULL, sink, " " );
   astWrite( ch, obj );
   ch = astAnnul( ch );
   return dump;
}

static void str( const char *s ) {
   putchar( '"' );
   for( ; *s; s++ ) {
      if( *s == '\n' ) {
         printf( "\\n" );
      } else {
         if( *s == '"' || *s == '\\' ) putchar( '\\' );
         putchar( *s );
      }
   }
   putchar( '"' );
}

static void num( double v ) {
   char buf[ 40 ];
   sprintf( buf, "%.17g", v );
   printf( "%s%s", buf, strpbrk( buf, ".en" ) ? "" : ".0" );
}

static uint64_t fnv( const char *s ) {
   uint64_t h = 0xcbf29ce484222325ULL;
   for( ; *s; s++ ) {
      h ^= (unsigned char) *s;
      h *= 0x100000001b3ULL;
   }
   return h;
}

/* One Moc: `(name, MaxOrder, MocLength, MocArea, dump hash, dump or "")`. */
static void emit( const char *name, AstMoc *moc ) {
   const char *d = dumped( (AstObject *) moc );
   printf( "    (\"%s\", %d, %d, ", name, astGetI( moc, "MaxOrder" ),
           astGetI( moc, "MocLength" ) );
   num( astGetD( moc, "MocArea" ) );
   printf( ", 0x%016llx, ", (unsigned long long) fnv( d ) );
   if( strlen( d ) < 6000 ) {
      str( d );
   } else {
      printf( "\"\"" );
   }
   printf( "),\n" );
}

static const char *header[] = {
   "NAXIS   = 2",
   "NAXIS1  = 20",
   "NAXIS2  = 15",
   "CTYPE1  = 'RA---TAN'",
   "CTYPE2  = 'DEC--TAN'",
   "CRPIX1  = 10.0",
   "CRPIX2  = 8.0",
   "CRVAL1  = 30.0",
   "CRVAL2  = 20.0",
   "CDELT1  = -0.02",
   "CDELT2  = 0.02",
   "RADESYS = 'ICRS'",
   NULL
};

static AstFrameSet *wcs( int swapped ) {
   AstFitsChan *fc;
   AstFrameSet *fs;
   char card[ 81 ];
   int i;
   fc = astFitsChan( NULL, NULL, " " );
   for( i = 0; header[ i ]; i++ ) {
      strcpy( card, header[ i ] );
      if( swapped && !strncmp( card, "CTYPE1", 6 ) ) strcpy( card, "CTYPE1  = 'DEC--TAN'" );
      if( swapped && !strncmp( card, "CTYPE2", 6 ) ) strcpy( card, "CTYPE2  = 'RA---TAN'" );
      if( swapped && !strncmp( card, "CRVAL1", 6 ) ) strcpy( card, "CRVAL1  = 20.0" );
      if( swapped && !strncmp( card, "CRVAL2", 6 ) ) strcpy( card, "CRVAL2  = 30.0" );
      astPutFits( fc, card, 0 );
   }
   astClear( fc, "Card" );
   fs = astRead( fc );
   fc = astAnnul( fc );
   return fs;
}

static AstCircle *circle( AstSkyFrame *sf, double x, double y, double r ) {
   double c[ 2 ];
   c[ 0 ] = x;
   c[ 1 ] = y;
   return astCircle( sf, 1, c, &r, NULL, " " );
}

static AstMoc *ordered( int maxorder, int minorder ) {
   AstMoc *moc = astMoc( " " );
   if( maxorder >= 0 ) astSetI( moc, "MaxOrder", maxorder );
   if( minorder >= 0 ) astSetI( moc, "MinOrder", minorder );
   return moc;
}

int main( void ) {
   int status = 0;
   int i, ix, iy;
   int dims[ 2 ] = { NX, NY };
   int iarr[ NX*NY ];
   float farr[ NX*NY ];
   double darr[ NX*NY ];
   double poly[ 6 ] = { 1.0, 1.5, 1.4, 0.2, 0.1, 0.6 };
   double bc[ 3 ] = { 5000.0, 1.0, 0.2 };
   double bk[ 3 ] = { 5100.0, 1.1, 0.3 };
   AstSkyFrame *icrs, *gal;
   AstCmpFrame *cf;
   AstRegion *neg;
   AstMoc *m, *c1, *c2, *c4, *empty;
   AstFrameSet *fs, *fsw;
   AstFrame *bf;

   astWatch( &status );
   for( iy = 0; iy < NY; iy++ ) {
      for( ix = 0; ix < NX; ix++ ) {
         i = ix + iy*NX;
         darr[ i ] = ( ix - 8.0 )*( ix - 8.0 )/16.0 + ( iy - 6.0 )*( iy - 6.0 )/9.0;
         farr[ i ] = (float)( 0.3*ix - 0.2*iy );
         iarr[ i ] = ( ix + 2*iy ) % 5;
      }
   }
   darr[ 3 + 4*NX ] = -999.0;
   darr[ 9 + 6*NX ] = -999.0;

   icrs = astSkyFrame( "System=ICRS" );
   gal = astSkyFrame( "System=Galactic" );

   printf( "// Generated by tools/oracle_fixtures/region_moc_add_probe.c. Do not edit.\n\n" );
   printf( "/// The FITS header of the pixel masks' WCS, one card per line.\n" );
   printf( "pub const HEADER: &str = \"" );
   for( i = 0; header[ i ]; i++ ) printf( "%s\\n", header[ i ] );
   printf( "\";\n\n" );
   printf( "/// `(case, MaxOrder, MocLength, MocArea, FNV-1a hash of the dump, the dump\n" );
   printf( "/// if short or \"\")`.\n" );
   printf( "pub type Case = (&'static str, i32, i32, f64, u64, &'static str);\n" );
   printf( "pub const CASES: &[Case] = &[\n" );

   c1 = ordered( 7, -1 );
   astAddRegion( c1, AST__OR, circle( icrs, 1.2, 0.3, 0.2 ) );
   emit( "circle", c1 );

   c2 = astCopy( c1 );
   astAddRegion( c2, AST__XOR, astPolygon( icrs, 3, 3, poly, NULL, " " ) );
   emit( "circle_polygon_xor", c2 );

   m = astCopy( c1 );
   neg = (AstRegion *) circle( icrs, 1.3, 0.35, 0.1 );
   astNegate( neg );
   astAddRegion( m, AST__AND, neg );
   emit( "and_negated", m );

   c4 = ordered( 6, -1 );
   astAddRegion( c4, AST__OR, circle( icrs, 0.5, 1.45, 0.2 ) );
   emit( "pole", c4 );

   m = ordered( 6, -1 );
   astAddRegion( m, AST__OR, circle( gal, 2.0, -0.5, 0.3 ) );
   emit( "galactic", m );

   m = ordered( 6, 3 );
   cf = astCmpFrame( astSpecFrame( " " ), icrs, " " );
   astAddRegion( m, AST__OR, astBox( cf, 1, bc, bk, NULL, " " ) );
   emit( "cmpframe_box", m );

   m = ordered( 5, 6 );
   astAddRegion( m, AST__OR, circle( icrs, 1.2, 0.3, 0.2 ) );
   emit( "oversample", m );

   m = ordered( 5, 5 );
   astAddRegion( m, AST__OR, circle( icrs, 1.2, 0.3, 0.2 ) );
   emit( "equal_orders", m );

   m = ordered( 5, 6 );
   neg = (AstRegion *) circle( icrs, 1.2, 0.3, 0.2 );
   astNegate( neg );
   astAddRegion( m, AST__OR, neg );
   emit( "oversample_negated", m );

   m = ordered( 5, -1 );
   astAddRegion( m, AST__OR, c2 );
   emit( "moc_lower", m );

   m = ordered( 9, -1 );
   astAddRegion( m, AST__OR, c1 );
   neg = astCopy( c4 );
   astNegate( neg );
   astAddRegion( m, AST__XOR, neg );
   emit( "moc_higher_negated", m );

   m = ordered( -1, -1 );
   astAddRegion( m, AST__OR, c2 );
   emit( "moc_unset", m );

   m = astCopy( c1 );
   empty = astMoc( " " );
   astAddRegion( m, AST__AND, empty );
   emit( "moc_and_empty", m );

   m = ordered( -1, -1 );
   astAddRegion( m, AST__OR, circle( icrs, 4.0, -0.6, 0.05 ) );
   emit( "auto_order", m );

   fs = wcs( 0 );
   fsw = wcs( 1 );

   m = ordered( -1, -1 );
   astAddPixelMaskD( m, AST__OR, fs, 4.0, AST__GT, 0, 0.0, darr, dims );
   emit( "mask_gt_d_auto", m );

   m = ordered( 11, 8 );
   astAddPixelMaskD( m, AST__OR, fs, 3.0, AST__LE, AST__USEBAD, -999.0, darr, dims );
   emit( "mask_le_d_bad", m );

   m = ordered( 13, 12 );
   astAddPixelMaskI( m, AST__OR, fs, 2, AST__EQ, 0, 0, iarr, dims );
   emit( "mask_eq_i", m );
   astAddPixelMaskI( m, AST__AND, fs, 0, AST__NE, 0, 0, iarr, dims );
   emit( "mask_eq_i_and_ne", m );

   m = ordered( 10, 9 );
   astAddPixelMaskF( m, AST__XOR, fs, 1.0f, AST__LT, 0, 0.0f, farr, dims );
   emit( "mask_lt_f_xor", m );

   m = ordered( 11, 8 );
   astAddPixelMaskD( m, AST__OR, fsw, 5.0, AST__GE, 0, 0.0, darr, dims );
   emit( "mask_ge_d_swapped", m );

   m = astCopy( c1 );
   astAddPixelMaskD( m, AST__AND, fs, 1.0e9, AST__GT, 0, 0.0, darr, dims );
   emit( "mask_and_none", m );

   printf( "];\n\n" );

   bf = astFrame( 2, " " );
   {
      double c[ 2 ] = { 0.0, 0.0 }, r = 1.0;
      m = astMoc( " " );
      astAddRegion( m, AST__OR, astCircle( bf, 1, c, &r, NULL, " " ) );
   }
   printf( "/// The status of adding a Region with no sky axes.\n" );
   printf( "pub const NO_SKY: i32 = %d;\n", status );
   astClearStatus;

   m = ordered( 0, -1 );
   astAddRegion( m, AST__OR, circle( icrs, 1.2, 0.3, 0.2 ) );
   printf( "/// The status of adding a Region with MaxOrder 0.\n" );
   printf( "pub const REGION_ORDER_ZERO: i32 = %d;\n", status );
   astClearStatus;

   m = ordered( 0, -1 );
   astAddPixelMaskD( m, AST__OR, fs, 4.0, AST__GT, 0, 0.0, darr, dims );
   printf( "/// The status of adding a pixel mask with MaxOrder 0.\n" );
   printf( "pub const MASK_ORDER_ZERO: i32 = %d;\n", status );
   astClearStatus;

   return 0;
}
