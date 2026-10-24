/* rebin_probe -- capture astRebin<X> (mapping.c) for every data type it
 * exists for (D, F, I, B, UB) and every spreading scheme: nearest, linear
 * and the 1-d kernels. 1-D, 2-D and 3-D grids, linear and non-linear
 * Mappings, with and without a linear-fit tolerance and with a small
 * initial scale size, inputs large enough to be simplified and rebinned in
 * blocks, kernels overlapping the output's edges, bad input pixels and
 * variances (AST__USEBAD), output variances (AST__USEVAR), variance arrays
 * given without AST__USEVAR, a weight limit
 * (wlim), and an input region smaller than the input grid. Each case
 * records the status, an FNV-1a hash of the bit patterns of every output
 * data and variance value converted to double, and every 97th output value.
 * Also the status for invalid arguments, with the first two output values
 * left after each, which show how much of the output C zeroes.
 *
 * Input pixel k (in vector order) holds (7k mod 23) - 11 for a signed
 * integer type, (7k mod 23) for an unsigned one, and (7k mod 97)*0.37 - 10
 * for a floating one; its variance (3k mod 11) and (3k mod 11)*0.5 + 0.25.
 * With AST__USEBAD, pixels with k mod 17 = 3 are bad, as are variances with
 * k mod 19 = 5. The bad value is 120. Output arrays start as 77.
 *
 * Prints Rust source for crates/starlink-ast/tests/rebin_oracle/cases.rs:
 *
 *   AST_REPLACE_NAN=1 ./rebin_probe > ../../crates/starlink-ast/tests/rebin_oracle/cases.rs
 */
/* The input values are formed as the port forms them, without fused
   multiply-adds. */
#pragma STDC FP_CONTRACT OFF

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"

static const char *spread_name( int spread ) {
   switch( spread ) {
      case AST__NEAREST: return "nearest";
      case AST__LINEAR: return "linear";
      case AST__SINC: return "sinc";
      case AST__SINCSINC: return "sincsinc";
      case AST__SINCCOS: return "sinccos";
      case AST__SINCGAUSS: return "sincgauss";
      case AST__GAUSS: return "gauss";
      case AST__SOMB: return "somb";
      case AST__SOMBCOS: return "sombcos";
   }
   return "?";
}

static AstMapping *mapping( const char *name ) {
   if( !strcmp( name, "unit2" ) ) return (AstMapping *) astUnitMap( 2, " " );
   if( !strcmp( name, "shift_half" ) ) {
      double sh[ 2 ] = { 0.5, -0.25 };
      return (AstMapping *) astShiftMap( 2, sh, " " );
   }
   if( !strcmp( name, "zoom2" ) ) return (AstMapping *) astZoomMap( 2, 1.7, " " );
   if( !strcmp( name, "shrink2" ) ) return (AstMapping *) astZoomMap( 2, 0.45, " " );
   if( !strcmp( name, "perm" ) ) {
      int p[ 2 ] = { 2, 1 };
      return (AstMapping *) astPermMap( 2, p, 2, p, NULL, " " );
   }
   if( !strcmp( name, "nonlin2" ) ) {
      const char *f[] = { "p=a+0.02*a*b-0.3", "q=b-0.015*a*a+0.2" };
      const char *i[] = { "a=p", "b=q" };
      return (AstMapping *) astMathMap( 2, 2, 2, f, 2, i, " " );
   }
   if( !strcmp( name, "cubic1" ) ) {
      const char *f[] = { "p=0.001*a*a*a+0.5*a" };
      const char *i[] = { "a=p" };
      return (AstMapping *) astMathMap( 1, 1, 1, f, 1, i, " " );
   }
   if( !strcmp( name, "win3" ) ) {
      double a1[ 3 ] = { 0.0, 0.0, 0.0 }, a2[ 3 ] = { 1.0, 1.0, 1.0 };
      double b1[ 3 ] = { 0.3, -0.2, 0.1 }, b2[ 3 ] = { 1.1, 0.7, 1.4 };
      return (AstMapping *) astWinMap( 3, a1, a2, b1, b2, " " );
   }
   if( !strcmp( name, "nonlin3" ) ) {
      const char *f[] = { "p=a+0.05*b*c", "q=b-0.03*a*a", "r=0.9*c+0.1*a" };
      const char *i[] = { "a=p", "b=q", "c=r" };
      return (AstMapping *) astMathMap( 3, 3, 3, f, 3, i, " " );
   }
   if( !strcmp( name, "noforward" ) ) {
      const char *f[] = { "p", "q" };
      const char *i[] = { "a=p", "b=q" };
      return (AstMapping *) astMathMap( 2, 2, 2, f, 2, i, " " );
   }
   return NULL;
}

typedef struct Case {
   const char *name, *map;
   int ndim_in, ndim_out;
   AstDim lbnd_in[ 3 ], ubnd_in[ 3 ], lbnd_out[ 3 ], ubnd_out[ 3 ], lbnd[ 3 ], ubnd[ 3 ];
   int spread, flags, usevar, maxpix;
   double tol, wlim;
   double params[ 2 ];
} Case;

static void dims( int n, const AstDim *a ) {
   int i;
   printf( "&[" );
   for( i = 0; i < n; i++ ) printf( "%s%lld", i ? ", " : "", (long long) a[ i ] );
   printf( "]" );
}

static void dbl( double v ) {
   char buf[ 40 ];
   if( v != v ) {
      printf( "f64::NAN" );
      return;
   }
   if( v > 1.0e308 * 10.0 || v < -1.0e308 * 10.0 ) {
      printf( "%sf64::INFINITY", v < 0.0 ? "-" : "" );
      return;
   }
   snprintf( buf, sizeof( buf ), "%.17g", v );
   printf( "%s%s", buf, strpbrk( buf, ".en" ) ? "" : ".0" );
}

static AstDim npix( int n, const AstDim *l, const AstDim *u ) {
   AstDim r = 1;
   int i;
   for( i = 0; i < n; i++ ) r *= u[ i ] - l[ i ] + 1;
   return r;
}

#define RUN(X,Xtype,Floating,Signed) \
static void run_##X( const Case *c ) { \
   AstMapping *m = mapping( c->map ); \
   AstDim nin = npix( c->ndim_in, c->lbnd_in, c->ubnd_in ); \
   AstDim nout = npix( c->ndim_out, c->lbnd_out, c->ubnd_out ); \
   Xtype *in = malloc( sizeof( Xtype ) * nin ), *in_var = malloc( sizeof( Xtype ) * nin ); \
   Xtype *out = malloc( sizeof( Xtype ) * nout ), *out_var = malloc( sizeof( Xtype ) * nout ); \
   Xtype badval = (Xtype) 120; \
   AstDim k; \
   uint64_t hash = 14695981039346656037ULL, bits; \
   double d; \
   int status; \
   for( k = 0; k < nin; k++ ) { \
      if( Floating ) { \
         in[ k ] = (Xtype) ( ( ( 7 * k ) % 97 ) * 0.37 - 10.0 ); \
         in_var[ k ] = (Xtype) ( ( ( 3 * k ) % 11 ) * 0.5 + 0.25 ); \
      } else { \
         in[ k ] = (Xtype) ( ( ( 7 * k ) % 23 ) - ( Signed ? 11 : 0 ) ); \
         in_var[ k ] = (Xtype) ( ( 3 * k ) % 11 ); \
      } \
      if( ( c->flags & AST__USEBAD ) && k % 17 == 3 ) in[ k ] = badval; \
      if( ( c->flags & AST__USEBAD ) && k % 19 == 5 ) in_var[ k ] = badval; \
   } \
   for( k = 0; k < nout; k++ ) out[ k ] = out_var[ k ] = (Xtype) 77; \
   astRebin8##X( m, c->wlim, c->ndim_in, c->lbnd_in, c->ubnd_in, in, \
                 c->usevar ? in_var : NULL, c->spread, c->params, \
                 c->flags, c->tol, c->maxpix, badval, c->ndim_out, \
                 c->lbnd_out, c->ubnd_out, c->lbnd, c->ubnd, out, \
                 c->usevar ? out_var : NULL ); \
   status = astStatus; \
   astClearStatus; \
   for( k = 0; k < nout; k++ ) { \
      d = (double) out[ k ]; \
      memcpy( &bits, &d, 8 ); \
      hash = ( hash ^ bits ) * 1099511628211ULL; \
      if( c->usevar ) { \
         d = (double) out_var[ k ]; \
         memcpy( &bits, &d, 8 ); \
         hash = ( hash ^ bits ) * 1099511628211ULL; \
      } \
   } \
   printf( "    (\"%s\", \"%s\", \"" #X "\", %d, ", c->name, c->map, c->ndim_in ); \
   dims( c->ndim_in, c->lbnd_in ); printf( ", " ); \
   dims( c->ndim_in, c->ubnd_in ); printf( ", " ); \
   dims( c->ndim_out, c->lbnd_out ); printf( ", " ); \
   dims( c->ndim_out, c->ubnd_out ); printf( ", " ); \
   dims( c->ndim_in, c->lbnd ); printf( ", " ); \
   dims( c->ndim_in, c->ubnd ); \
   printf( ", \"%s\", %s, %s, %s, [", spread_name( c->spread ), \
           ( c->flags & AST__USEBAD ) ? "true" : "false", \
           ( c->flags & AST__USEVAR ) ? "true" : "false", \
           c->usevar ? "true" : "false" ); \
   dbl( c->params[ 0 ] ); printf( ", " ); dbl( c->params[ 1 ] ); printf( "], " ); \
   dbl( c->tol ); printf( ", %d, ", c->maxpix ); dbl( c->wlim ); \
   printf( ", %d, %llu, &[", status, (unsigned long long) hash ); \
   for( k = 0; k < nout; k += 97 ) { \
      printf( "%s", k ? ", " : "" ); \
      dbl( (double) out[ k ] ); \
   } \
   printf( "]),\n" ); \
   free( in ); free( in_var ); free( out ); free( out_var ); \
}

RUN(D,double,1,1)
RUN(F,float,1,1)
RUN(I,int,0,1)
RUN(B,signed char,0,1)
RUN(UB,unsigned char,0,0)

int main( void ) {
   int status = 0, i;
   static const Case cases[] = {
      { "unit_nearest", "unit2", 2, 2, {-3,2}, {16,16}, {-3,2}, {16,16}, {-3,2}, {16,16},
        AST__NEAREST, 0, 0, 50, 0.0, 0.0 },
      { "unit_linear_var", "unit2", 2, 2, {-3,2}, {16,16}, {-3,2}, {16,16}, {-3,2}, {16,16},
        AST__LINEAR, AST__USEVAR, 1, 50, 0.0, 0.0 },
      { "shift_linear_fit", "shift_half", 2, 2, {-3,2}, {16,16}, {-5,0}, {18,18}, {-3,2}, {16,16},
        AST__LINEAR, AST__USEBAD | AST__USEVAR, 1, 50, 0.1, 0.0 },
      { "shift_nearest_bad", "shift_half", 2, 2, {-3,2}, {16,16}, {-5,0}, {18,18}, {-3,2}, {16,16},
        AST__NEAREST, AST__USEBAD | AST__USEVAR, 1, 50, 0.1, 0.0 },
      { "zoom_big", "zoom2", 2, 2, {-3,2}, {36,36}, {-10,-5}, {65,65}, {-3,2}, {36,36},
        AST__LINEAR, AST__USEBAD, 0, 1000, 0.1, 0.0 },
      { "shrink_nearest", "shrink2", 2, 2, {-3,2}, {16,16}, {-3,-1}, {9,9}, {-3,2}, {16,16},
        AST__NEAREST, AST__USEVAR, 1, 50, 0.1, 0.0 },
      { "perm_region", "perm", 2, 2, {-3,2}, {16,16}, {0,-5}, {20,20}, {0,4}, {12,11},
        AST__LINEAR, AST__USEBAD | AST__USEVAR, 1, 50, 0.0, 0.0 },
      { "nonlin_divided", "nonlin2", 2, 2, {-3,2}, {16,16}, {-6,-2}, {20,22}, {-3,2}, {16,16},
        AST__LINEAR, AST__USEVAR, 1, 8, 0.01, 0.0 },
      { "nonlin_nearest", "nonlin2", 2, 2, {-3,2}, {16,16}, {-6,-2}, {20,22}, {-3,2}, {16,16},
        AST__NEAREST, AST__USEBAD, 0, 50, 0.0, 0.0 },
      { "cubic_linear", "cubic1", 1, 1, {-30}, {40}, {-50}, {80}, {-30}, {40},
        AST__LINEAR, AST__USEBAD | AST__USEVAR, 1, 20, 0.001, 0.0 },
      { "cubic_nearest", "cubic1", 1, 1, {-30}, {40}, {-50}, {80}, {-30}, {40},
        AST__NEAREST, 0, 0, 20, 0.0, 0.0 },
      { "win3_linear", "win3", 3, 3, {-2,0,1}, {6,7,5}, {-3,-1,0}, {9,8,9}, {-2,0,1}, {6,7,5},
        AST__LINEAR, AST__USEBAD | AST__USEVAR, 1, 50, 0.1, 0.0 },
      { "win3_nearest", "win3", 3, 3, {-2,0,1}, {6,7,5}, {-3,-1,0}, {9,8,9}, {-2,0,1}, {6,7,5},
        AST__NEAREST, AST__USEVAR, 1, 50, 0.1, 0.0 },
      { "nonlin3_linear", "nonlin3", 3, 3, {-2,0,1}, {6,7,5}, {-3,-1,0}, {9,8,9}, {-2,0,1}, {6,7,5},
        AST__LINEAR, 0, 0, 50, 0.0, 0.0 },
      { "sinc_shift", "shift_half", 2, 2, {-3,2}, {16,16}, {-5,0}, {18,18}, {-3,2}, {16,16},
        AST__SINC, AST__USEBAD | AST__USEVAR, 1, 50, 0.1, 0.0, {2.0, 2.0} },
      { "sinc_unit_edges", "unit2", 2, 2, {-3,2}, {16,16}, {-3,2}, {16,16}, {-3,2}, {16,16},
        AST__SINC, 0, 0, 50, 0.0, 0.0, {0.0, 0.0} },
      { "sincsinc_nonlin", "nonlin2", 2, 2, {-3,2}, {16,16}, {-6,-2}, {20,22}, {-3,2}, {16,16},
        AST__SINCSINC, AST__USEVAR, 1, 8, 0.01, 0.0, {0.0, 2.5} },
      { "sinccos_cubic", "cubic1", 1, 1, {-30}, {40}, {-50}, {80}, {-30}, {40},
        AST__SINCCOS, AST__USEBAD | AST__USEVAR, 1, 20, 0.001, 0.0, {3.0, 2.0} },
      { "sincgauss_zoom", "zoom2", 2, 2, {-3,2}, {16,16}, {-10,-5}, {30,30}, {-3,2}, {16,16},
        AST__SINCGAUSS, 0, 0, 50, 0.1, 0.0, {0.0, 1.5} },
      { "gauss_shrink", "shrink2", 2, 2, {-3,2}, {16,16}, {-3,-1}, {9,9}, {-3,2}, {16,16},
        AST__GAUSS, AST__USEBAD | AST__USEVAR, 1, 50, 0.1, 0.0, {2.0, 1.2} },
      { "gauss_tiny_out", "unit2", 2, 2, {-3,2}, {16,16}, {0,4}, {2,6}, {-3,2}, {16,16},
        AST__GAUSS, 0, 1, 50, 0.0, 0.0, {3.0, 2.0} },
      { "somb_unit", "unit2", 2, 2, {-3,2}, {16,16}, {-3,2}, {16,16}, {-3,2}, {16,16},
        AST__SOMB, AST__USEBAD, 0, 50, 0.0, 0.0, {2.0, 2.0} },
      { "sombcos_win3", "win3", 3, 3, {-2,0,1}, {6,7,5}, {-3,-1,0}, {9,8,9}, {-2,0,1}, {6,7,5},
        AST__SOMBCOS, AST__USEBAD | AST__USEVAR, 1, 50, 0.1, 0.0, {2.0, 2.0} },
      { "sinc_nonlin3_edges", "nonlin3", 3, 3, {-2,0,1}, {6,7,5}, {0,1,2}, {4,5,4}, {-2,0,1}, {6,7,5},
        AST__SINC, AST__USEVAR, 1, 50, 0.0, 0.0, {2.0, 0.0} },
      { "gauss_cubic_edges", "cubic1", 1, 1, {-30}, {40}, {-8}, {9}, {-30}, {40},
        AST__GAUSS, AST__USEVAR, 1, 20, 0.0, 0.0, {3.0, 1.5} },
      { "wlim_linear", "zoom2", 2, 2, {-3,2}, {16,16}, {-10,-5}, {30,30}, {-3,2}, {16,16},
        AST__LINEAR, AST__USEVAR, 1, 50, 0.1, 0.3 },
      { "wlim_sinc", "shift_half", 2, 2, {-3,2}, {16,16}, {-5,0}, {18,18}, {-3,2}, {16,16},
        AST__SINC, AST__USEBAD, 0, 50, 0.1, 0.9, {2.0, 0.0} },
   };
   astWatch( &status );
   printf( "// Generated by tools/oracle_fixtures/rebin_probe.c. Do not edit.\n\n" );
   printf( "/// `(case, Mapping, type, ndim_in, lbnd_in, ubnd_in, lbnd_out, ubnd_out,\n" );
   printf( "/// region lbnd, region ubnd, spread, use_bad, use_var, variance arrays\n" );
   printf( "/// given, params, tol, maxpix, wlim, status, FNV-1a hash, every 97th output\n" );
   printf( "/// value)`.\n" );
   printf( "pub type Case = (&'static str, &'static str, &'static str, usize,\n" );
   printf( "    &'static [i64], &'static [i64], &'static [i64], &'static [i64],\n" );
   printf( "    &'static [i64], &'static [i64], &'static str, bool, bool, bool, [f64; 2],\n" );
   printf( "    f64, usize, f64, i32, u64, &'static [f64]);\n" );
   printf( "pub const CASES: &[Case] = &[\n" );
   for( i = 0; i < (int) ( sizeof( cases ) / sizeof( cases[ 0 ] ) ); i++ ) {
      run_D( cases + i ); run_F( cases + i ); run_I( cases + i );
      run_B( cases + i ); run_UB( cases + i );
   }
   printf( "];\n\n" );

   /* Errors, all for double data, with the output starting as 7. */
   {
      double in[ 400 ], out[ 400 ];
      AstDim l2[ 2 ] = { 1, 1 }, u2[ 2 ] = { 10, 10 }, bad[ 2 ] = { 11, 1 };
      AstDim rl[ 2 ] = { 0, 1 }, ru[ 2 ] = { 10, 11 }, rx[ 2 ] = { 5, 1 }, ry[ 2 ] = { 4, 10 };
      AstMapping *u = mapping( "unit2" ), *n = mapping( "noforward" );
      int codes[ 10 ];
      double first[ 10 ][ 2 ];
      for( i = 0; i < 400; i++ ) in[ i ] = i;
#define TRY(j, call) for( i = 0; i < 400; i++ ) out[ i ] = 7.0; \
      call; codes[ j ] = astStatus; astClearStatus; \
      first[ j ][ 0 ] = out[ 0 ]; first[ j ][ 1 ] = out[ 1 ];
      TRY( 0, astRebin8D( u, 0.0, 1, l2, u2, in, NULL, AST__LINEAR, NULL, 0, 0.0, 50, -1.0, 2, l2, u2, l2, u2, out, NULL ) )
      TRY( 1, astRebin8D( u, 0.0, 2, l2, u2, in, NULL, AST__LINEAR, NULL, 0, 0.0, 50, -1.0, 3, l2, u2, l2, u2, out, NULL ) )
      TRY( 2, astRebin8D( u, 0.0, 2, bad, u2, in, NULL, AST__LINEAR, NULL, 0, 0.0, 50, -1.0, 2, l2, u2, l2, u2, out, NULL ) )
      TRY( 3, astRebin8D( u, 0.0, 2, l2, u2, in, NULL, AST__LINEAR, NULL, 0, 0.0, 50, -1.0, 2, bad, u2, l2, u2, out, NULL ) )
      TRY( 4, astRebin8D( u, 0.0, 2, l2, u2, in, NULL, AST__LINEAR, NULL, 0, 0.0, 50, -1.0, 2, l2, u2, rl, u2, out, NULL ) )
      TRY( 5, astRebin8D( u, 0.0, 2, l2, u2, in, NULL, AST__LINEAR, NULL, 0, 0.0, 50, -1.0, 2, l2, u2, l2, ru, out, NULL ) )
      TRY( 6, astRebin8D( u, 0.0, 2, l2, u2, in, NULL, AST__LINEAR, NULL, 0, 0.0, 50, -1.0, 2, l2, u2, rx, ry, out, NULL ) )
      TRY( 7, astRebin8D( u, 0.0, 2, l2, u2, in, NULL, AST__LINEAR, NULL, 0, -1.0, 50, -1.0, 2, l2, u2, l2, u2, out, NULL ) )
      TRY( 8, astRebin8D( n, 0.0, 2, l2, u2, in, NULL, AST__LINEAR, NULL, 0, 0.0, 50, -1.0, 2, l2, u2, l2, u2, out, NULL ) )
      TRY( 9, astRebin8D( u, 0.0, 2, l2, u2, in, NULL, AST__LINEAR, NULL, 0, 0.0, 50, -1.0, 2, l2, u2, l2, u2, out, NULL ) )
      printf( "/// `(status, first two output values)` for: too few input dimensions,\n" );
      printf( "/// too many output dimensions, an input lower bound above its upper bound,\n" );
      printf( "/// the same for the output grid, a region below and above the input grid,\n" );
      printf( "/// a region with a lower bound above its upper bound, a negative\n" );
      printf( "/// tolerance, a Mapping with no forward transformation, and no error. The\n" );
      printf( "/// output starts as 7.\n" );
      printf( "pub const ERRORS: [(i32, [f64; 2]); 10] = [\n" );
      for( i = 0; i < 10; i++ ) {
         printf( "    (%d, [", codes[ i ] );
         dbl( first[ i ][ 0 ] ); printf( ", " ); dbl( first[ i ][ 1 ] ); printf( "]),\n" );
      }
      printf( "];\n" );
   }
   return 0;
}
