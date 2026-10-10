/* resample_probe -- capture astResample<X> with nearest-neighbour and
 * linear interpolation (mapping.c), for every data type: 1-D, 2-D and 3-D
 * grids, linear and non-linear Mappings, with and without a linear-fit
 * tolerance and with a small initial scale size, outputs large enough to be
 * simplified and resampled in blocks, bad input pixels and variances
 * (AST__USEBAD), outputs left alone where bad (AST__NOBAD), variance arrays,
 * and an output region smaller than the output grid. Each case records the
 * number of bad output pixels, the status, an FNV-1a hash of the bit
 * patterns of every output data and variance value converted to double,
 * and every 211th output value. Also the status for invalid arguments.
 *
 * Input pixel k (in vector order) holds (7k mod 97) - 40 for a signed type,
 * (7k mod 97) for an unsigned one, and (7k mod 97)*0.37 - 10 for a floating
 * one; its variance (3k mod 11) - 2, (3k mod 11) and (3k mod 11)*0.5 - 1.
 * With AST__USEBAD, pixels with k mod 17 = 3 are bad, as are variances
 * with k mod 19 = 5. The bad value is 120. Output arrays start as 77.
 *
 * Prints Rust source for crates/ast-core/tests/resample_oracle/cases.rs:
 *
 *   AST_REPLACE_NAN=1 ./resample_probe > ../../crates/ast-core/tests/resample_oracle/cases.rs
 */
/* The input values are formed as the port forms them, without fused
   multiply-adds. */
#pragma STDC FP_CONTRACT OFF

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"

static AstMapping *mapping( const char *name ) {
   if( !strcmp( name, "unit2" ) ) return (AstMapping *) astUnitMap( 2, " " );
   if( !strcmp( name, "shift_half" ) ) {
      double sh[ 2 ] = { 0.5, -0.25 };
      return (AstMapping *) astShiftMap( 2, sh, " " );
   }
   if( !strcmp( name, "zoom2" ) ) return (AstMapping *) astZoomMap( 2, 1.7, " " );
   if( !strcmp( name, "perm" ) ) {
      int p[ 2 ] = { 2, 1 };
      return (AstMapping *) astPermMap( 2, p, 2, p, NULL, " " );
   }
   if( !strcmp( name, "nonlin2" ) ) {
      const char *f[] = { "p=a", "q=b" };
      const char *i[] = { "a=p+0.02*p*q-0.3", "b=q-0.015*p*p+0.2" };
      return (AstMapping *) astMathMap( 2, 2, 2, f, 2, i, " " );
   }
   if( !strcmp( name, "cubic1" ) ) {
      const char *f[] = { "p=a" };
      const char *i[] = { "a=0.001*p*p*p+0.5*p" };
      return (AstMapping *) astMathMap( 1, 1, 1, f, 1, i, " " );
   }
   if( !strcmp( name, "win3" ) ) {
      double a1[ 3 ] = { 0.0, 0.0, 0.0 }, a2[ 3 ] = { 1.0, 1.0, 1.0 };
      double b1[ 3 ] = { 0.3, -0.2, 0.1 }, b2[ 3 ] = { 1.1, 0.7, 1.4 };
      return (AstMapping *) astWinMap( 3, a1, a2, b1, b2, " " );
   }
   if( !strcmp( name, "nonlin3" ) ) {
      const char *f[] = { "p=a", "q=b", "r=c" };
      const char *i[] = { "a=p+0.05*q*r", "b=q-0.03*p*p", "c=0.9*r+0.1*p" };
      return (AstMapping *) astMathMap( 3, 3, 3, f, 3, i, " " );
   }
   if( !strcmp( name, "noinverse" ) ) {
      const char *f[] = { "p=a", "q=b" };
      const char *i[] = { "a", "b" };
      return (AstMapping *) astMathMap( 2, 2, 2, f, 2, i, " " );
   }
   return NULL;
}

typedef struct Case {
   const char *name, *map;
   int ndim_in, ndim_out;
   AstDim lbnd_in[ 3 ], ubnd_in[ 3 ], lbnd_out[ 3 ], ubnd_out[ 3 ], lbnd[ 3 ], ubnd[ 3 ];
   int interp, flags, usevar, maxpix;
   double tol;
} Case;

static void dims( int n, const AstDim *a ) {
   int i;
   printf( "&[" );
   for( i = 0; i < n; i++ ) printf( "%s%lld", i ? ", " : "", (long long) a[ i ] );
   printf( "]" );
}

static void dbl( double v ) {
   char buf[ 40 ];
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
   AstDim k, nbad; \
   uint64_t hash = 14695981039346656037ULL, bits; \
   double d; \
   int status; \
   for( k = 0; k < nin; k++ ) { \
      if( Floating ) { \
         in[ k ] = (Xtype) ( ( ( 7 * k ) % 97 ) * 0.37 - 10.0 ); \
         in_var[ k ] = (Xtype) ( ( ( 3 * k ) % 11 ) * 0.5 - 1.0 ); \
      } else { \
         in[ k ] = (Xtype) ( ( ( 7 * k ) % 97 ) - ( Signed ? 40 : 0 ) ); \
         in_var[ k ] = (Xtype) ( ( ( 3 * k ) % 11 ) - ( Signed ? 2 : 0 ) ); \
      } \
      if( ( c->flags & AST__USEBAD ) && k % 17 == 3 ) in[ k ] = badval; \
      if( ( c->flags & AST__USEBAD ) && k % 19 == 5 ) in_var[ k ] = badval; \
   } \
   for( k = 0; k < nout; k++ ) out[ k ] = out_var[ k ] = (Xtype) 77; \
   nbad = astResample8##X( m, c->ndim_in, c->lbnd_in, c->ubnd_in, in, \
                           c->usevar ? in_var : NULL, c->interp, NULL, NULL, \
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
   dims( c->ndim_out, c->lbnd ); printf( ", " ); \
   dims( c->ndim_out, c->ubnd ); \
   printf( ", %s, %s, %s, %s, %s, ", c->interp == AST__NEAREST ? "\"nearest\"" : "\"linear\"", \
           ( c->flags & AST__USEBAD ) ? "true" : "false", \
           ( c->flags & AST__NOBAD ) ? "true" : "false", \
           c->usevar ? "true" : "false", "0" ); \
   dbl( c->tol ); \
   printf( ", %d, %lld, %d, %llu, &[", c->maxpix, (long long) nbad, status, \
           (unsigned long long) hash ); \
   for( k = 0; k < nout; k += 211 ) { \
      printf( "%s", k ? ", " : "" ); \
      dbl( (double) out[ k ] ); \
   } \
   printf( "]),\n" ); \
   free( in ); free( in_var ); free( out ); free( out_var ); \
}

RUN(D,double,1,1)
RUN(F,float,1,1)
RUN(L,long int,0,1)
RUN(UL,unsigned long int,0,0)
RUN(I,int,0,1)
RUN(UI,unsigned int,0,0)
RUN(S,short int,0,1)
RUN(US,unsigned short int,0,0)
RUN(B,signed char,0,1)
RUN(UB,unsigned char,0,0)

int main( void ) {
   int status = 0, i;
   static const Case cases[] = {
      { "unit_nearest", "unit2", 2, 2, {-3,2}, {16,16}, {-3,2}, {16,16}, {-3,2}, {16,16},
        AST__NEAREST, 0, 0, 50, 0.0 },
      { "unit_linear", "unit2", 2, 2, {-3,2}, {16,16}, {-3,2}, {16,16}, {-3,2}, {16,16},
        AST__LINEAR, 0, 0, 50, 0.0 },
      { "shift_linear_fit", "shift_half", 2, 2, {-3,2}, {16,16}, {-5,0}, {18,18}, {-5,0}, {18,18},
        AST__LINEAR, 0, 1, 50, 0.1 },
      { "shift_nearest_bad", "shift_half", 2, 2, {-3,2}, {16,16}, {-5,0}, {18,18}, {-5,0}, {18,18},
        AST__NEAREST, AST__USEBAD, 1, 50, 0.1 },
      { "zoom_big", "zoom2", 2, 2, {-3,2}, {16,16}, {-10,-5}, {49,44}, {-10,-5}, {49,44},
        AST__LINEAR, AST__USEBAD, 1, 1000, 0.1 },
      { "perm_nobad", "perm", 2, 2, {-3,2}, {16,16}, {0,-5}, {20,20}, {0,-5}, {20,20},
        AST__LINEAR, AST__USEBAD | AST__NOBAD, 1, 50, 0.0 },
      { "nonlin_divided", "nonlin2", 2, 2, {-3,2}, {16,16}, {-6,-2}, {20,22}, {-6,-2}, {20,22},
        AST__LINEAR, 0, 1, 8, 0.01 },
      { "nonlin_nearest", "nonlin2", 2, 2, {-3,2}, {16,16}, {-6,-2}, {20,22}, {-6,-2}, {20,22},
        AST__NEAREST, AST__USEBAD, 1, 50, 0.0 },
      { "cubic_linear", "cubic1", 1, 1, {-30}, {40}, {-50}, {60}, {-50}, {60},
        AST__LINEAR, AST__USEBAD, 1, 20, 0.001 },
      { "cubic_nearest", "cubic1", 1, 1, {-30}, {40}, {-50}, {60}, {-50}, {60},
        AST__NEAREST, 0, 0, 20, 0.0 },
      { "win3_linear", "win3", 3, 3, {-2,0,1}, {6,7,5}, {-3,-1,0}, {9,8,9}, {-3,-1,0}, {9,8,9},
        AST__LINEAR, AST__USEBAD, 1, 50, 0.1 },
      { "nonlin3_linear", "nonlin3", 3, 3, {-2,0,1}, {6,7,5}, {-3,-1,0}, {9,8,9}, {-3,-1,0}, {9,8,9},
        AST__LINEAR, 0, 1, 50, 0.0 },
      { "zoom_region", "zoom2", 2, 2, {-3,2}, {16,16}, {-10,-5}, {30,30}, {2,4}, {21,17},
        AST__LINEAR, AST__USEBAD, 1, 50, 0.1 },
   };
   astWatch( &status );
   printf( "// Generated by tools/oracle_fixtures/resample_probe.c. Do not edit.\n\n" );
   printf( "/// `(case, Mapping, type, ndim_in, lbnd_in, ubnd_in, lbnd_out, ubnd_out,\n" );
   printf( "/// region lbnd, region ubnd, interp, use_bad, no_bad, use_var, unused, tol,\n" );
   printf( "/// maxpix, bad pixels, status, FNV-1a hash, every 211th output value)`.\n" );
   printf( "pub type Case = (&'static str, &'static str, &'static str, usize,\n" );
   printf( "    &'static [i64], &'static [i64], &'static [i64], &'static [i64],\n" );
   printf( "    &'static [i64], &'static [i64], &'static str, bool, bool, bool, i32, f64,\n" );
   printf( "    usize, usize, i32, u64, &'static [f64]);\n" );
   printf( "pub const CASES: &[Case] = &[\n" );
   for( i = 0; i < (int) ( sizeof( cases ) / sizeof( cases[ 0 ] ) ); i++ ) {
      run_D( cases + i ); run_F( cases + i ); run_L( cases + i );
      run_UL( cases + i ); run_I( cases + i ); run_UI( cases + i );
      run_S( cases + i ); run_US( cases + i ); run_B( cases + i );
      run_UB( cases + i );
   }
   printf( "];\n\n" );

   /* Errors, all for double data. */
   {
      double in[ 400 ], out[ 400 ];
      AstDim l2[ 2 ] = { 1, 1 }, u2[ 2 ] = { 10, 10 }, bad[ 2 ] = { 11, 1 };
      AstDim rl[ 2 ] = { 0, 1 }, ru[ 2 ] = { 10, 11 };
      AstMapping *u = mapping( "unit2" ), *n = mapping( "noinverse" );
      int codes[ 8 ];
      for( i = 0; i < 400; i++ ) in[ i ] = i;
#define TRY(j, call) call; codes[ j ] = astStatus; astClearStatus;
      TRY( 0, astResample8D( u, 1, l2, u2, in, NULL, AST__LINEAR, NULL, NULL, 0, 0.0, 50, -1.0, 2, l2, u2, l2, u2, out, NULL ) )
      TRY( 1, astResample8D( u, 2, l2, u2, in, NULL, AST__LINEAR, NULL, NULL, 0, 0.0, 50, -1.0, 3, l2, u2, l2, u2, out, NULL ) )
      TRY( 2, astResample8D( u, 2, bad, u2, in, NULL, AST__LINEAR, NULL, NULL, 0, 0.0, 50, -1.0, 2, l2, u2, l2, u2, out, NULL ) )
      TRY( 3, astResample8D( u, 2, l2, u2, in, NULL, AST__LINEAR, NULL, NULL, 0, 0.0, 50, -1.0, 2, bad, u2, l2, u2, out, NULL ) )
      TRY( 4, astResample8D( u, 2, l2, u2, in, NULL, AST__LINEAR, NULL, NULL, 0, 0.0, 50, -1.0, 2, l2, u2, rl, u2, out, NULL ) )
      TRY( 5, astResample8D( u, 2, l2, u2, in, NULL, AST__LINEAR, NULL, NULL, 0, 0.0, 50, -1.0, 2, l2, u2, l2, ru, out, NULL ) )
      TRY( 6, astResample8D( u, 2, l2, u2, in, NULL, AST__LINEAR, NULL, NULL, 0, -1.0, 50, -1.0, 2, l2, u2, l2, u2, out, NULL ) )
      TRY( 7, astResample8D( n, 2, l2, u2, in, NULL, AST__LINEAR, NULL, NULL, 0, 0.0, 50, -1.0, 2, l2, u2, l2, u2, out, NULL ) )
      printf( "/// The status for: too few input dimensions, too many output\n" );
      printf( "/// dimensions, an input lower bound above its upper bound, the same for\n" );
      printf( "/// the output grid, a region below and above the output grid, a negative\n" );
      printf( "/// tolerance, and a Mapping with no inverse.\n" );
      printf( "pub const ERRORS: [i32; 8] = [%d, %d, %d, %d, %d, %d, %d, %d];\n", codes[ 0 ],
              codes[ 1 ], codes[ 2 ], codes[ 3 ], codes[ 4 ], codes[ 5 ], codes[ 6 ], codes[ 7 ] );
   }
   return 0;
}
