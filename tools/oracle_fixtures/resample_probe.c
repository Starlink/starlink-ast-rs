/* resample_probe -- capture astResample<X> (mapping.c) for every data type
 * and every interpolation scheme: nearest neighbour, linear, the 1-d
 * kernels, block averaging, a user kernel (tri_kernel below) and a user
 * interpolator (uinterp_<X> below), with and without flux conservation.
 * 1-D, 2-D and 3-D grids, linear and non-linear Mappings, with and without a linear-fit
 * tolerance and with a small initial scale size, outputs large enough to be
 * simplified and resampled in blocks, bad input pixels and variances
 * (AST__USEBAD), outputs left alone where bad (AST__NOBAD), variance arrays,
 * and an output region smaller than the output grid. Each case records the
 * number of bad output pixels, the status (a case may fail part way, having
 * written some of its output), an FNV-1a hash of the bit
 * patterns of every output data and variance value converted to double,
 * and every 211th output value. Also the status for invalid arguments and
 * for errors raised by flux conservation and by the user's functions.
 *
 * Input pixel k (in vector order) holds (7k mod 97) - 40 for a signed type,
 * (7k mod 97) for an unsigned one, and (7k mod 97)*0.37 - 10 for a floating
 * one; its variance (3k mod 11) - 2, (3k mod 11) and (3k mod 11)*0.5 - 1.
 * With AST__USEBAD, pixels with k mod 17 = 3 are bad, as are variances
 * with k mod 19 = 5. The bad value is 120. Output arrays start as 77.
 *
 * Prints Rust source for crates/starlink-ast/tests/resample_oracle/cases.rs:
 *
 *   AST_REPLACE_NAN=1 ./resample_probe > ../../crates/starlink-ast/tests/resample_oracle/cases.rs
 */
/* The input values are formed as the port forms them, without fused
   multiply-adds. */
#pragma STDC FP_CONTRACT OFF

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"

/* A user kernel: a triangle reaching two pixels either side, scaled by
   params[1]. It raises AST__BADIN for an offset of exactly 1.5 when
   params[1] is negative. */
static void tri_kernel( double offset, const double params[], int flags,
                        double *value ) {
   double a = offset < 0.0 ? -offset : offset;
   if( params[ 1 ] < 0.0 && a == 1.5 ) {
      astError( AST__BADIN, "tri_kernel: offset 1.5" );
      *value = 0.0;
      return;
   }
   *value = ( a < 2.0 ? 1.0 - 0.5 * a : 0.0 ) * ( params[ 1 ] < 0.0 ? 1.0 : params[ 1 ] );
}

/* A user interpolator: each output value is the sum over axes of
   params[0] times the input coordinate, truncated to the type, and the
   variance is the input value of the first pixel; a point whose first
   coordinate exceeds the input grid's upper bound is bad. It raises
   AST__BADIN when params[0] is negative. */
#define UINTERP(X,Xtype) \
static void uinterp_##X( int ndim_in, const AstDim lbnd_in[], const AstDim ubnd_in[], \
                         const Xtype in[], const Xtype in_var[], AstDim npoint, \
                         const AstDim offset[], const double *const coords[], \
                         const double params[], int flags, Xtype badval, \
                         Xtype *out, Xtype *out_var, AstDim *nbad ) { \
   AstDim point; \
   int idim; \
   double sum; \
   *nbad = 0; \
   if( params[ 0 ] < 0.0 ) { \
      astError( AST__BADIN, "uinterp: negative scale" ); \
      return; \
   } \
   for( point = 0; point < npoint; point++ ) { \
      if( coords[ 0 ][ point ] > ubnd_in[ 0 ] ) { \
         out[ offset[ point ] ] = badval; \
         if( out_var ) out_var[ offset[ point ] ] = badval; \
         ( *nbad )++; \
      } else { \
         sum = 0.0; \
         for( idim = 0; idim < ndim_in; idim++ ) sum += params[ 0 ] * coords[ idim ][ point ]; \
         out[ offset[ point ] ] = (Xtype) sum; \
         if( out_var ) out_var[ offset[ point ] ] = in[ 0 ]; \
      } \
   } \
}
UINTERP(D,double)
UINTERP(F,float)
UINTERP(L,long int)
UINTERP(UL,unsigned long int)
UINTERP(I,int)
UINTERP(UI,unsigned int)
UINTERP(S,short int)
UINTERP(US,unsigned short int)
UINTERP(B,signed char)
UINTERP(UB,unsigned char)

static const char *interp_name( int interp ) {
   switch( interp ) {
      case AST__NEAREST: return "nearest";
      case AST__LINEAR: return "linear";
      case AST__SINC: return "sinc";
      case AST__SINCSINC: return "sincsinc";
      case AST__SINCCOS: return "sinccos";
      case AST__SINCGAUSS: return "sincgauss";
      case AST__GAUSS: return "gauss";
      case AST__SOMB: return "somb";
      case AST__SOMBCOS: return "sombcos";
      case AST__BLOCKAVE: return "blockave";
      case AST__UKERN1: return "ukern1";
      case AST__UINTERP: return "uinterp";
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
   if( !strcmp( name, "zoom1" ) ) return (AstMapping *) astZoomMap( 1, 1.3, " " );
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
   void (*finterp)( void ) = c->interp == AST__UKERN1 ? (void (*)( void )) tri_kernel : \
                             c->interp == AST__UINTERP ? (void (*)( void )) uinterp_##X : NULL; \
   nbad = astResample8##X( m, c->ndim_in, c->lbnd_in, c->ubnd_in, in, \
                           c->usevar ? in_var : NULL, c->interp, finterp, c->params, \
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
   printf( ", \"%s\", %s, %s, %s, %s, [", interp_name( c->interp ), \
           ( c->flags & AST__USEBAD ) ? "true" : "false", \
           ( c->flags & AST__NOBAD ) ? "true" : "false", \
           c->usevar ? "true" : "false", \
           ( c->flags & AST__CONSERVEFLUX ) ? "true" : "false" ); \
   dbl( c->params[ 0 ] ); printf( ", " ); dbl( c->params[ 1 ] ); printf( "], " ); \
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
      { "sinc_shift", "shift_half", 2, 2, {-3,2}, {16,16}, {-5,0}, {18,18}, {-5,0}, {18,18},
        AST__SINC, AST__USEBAD, 1, 50, 0.1, {2.0, 2.0} },
      { "sinc_default", "unit2", 2, 2, {-3,2}, {16,16}, {-3,2}, {16,16}, {-3,2}, {16,16},
        AST__SINC, 0, 0, 50, 0.0, {0.0, 0.0} },
      { "sincsinc_nonlin", "nonlin2", 2, 2, {-3,2}, {16,16}, {-6,-2}, {20,22}, {-6,-2}, {20,22},
        AST__SINCSINC, 0, 1, 8, 0.01, {0.0, 2.5} },
      { "sinccos_cubic", "cubic1", 1, 1, {-30}, {40}, {-50}, {60}, {-50}, {60},
        AST__SINCCOS, AST__USEBAD, 1, 20, 0.001, {3.0, 2.0} },
      { "sincgauss_zoom", "zoom2", 2, 2, {-3,2}, {16,16}, {-10,-5}, {30,30}, {-10,-5}, {30,30},
        AST__SINCGAUSS, 0, 1, 50, 0.1, {0.0, 1.5} },
      { "gauss_nobad", "shift_half", 2, 2, {-3,2}, {16,16}, {-5,0}, {18,18}, {-5,0}, {18,18},
        AST__GAUSS, AST__USEBAD | AST__NOBAD, 1, 50, 0.1, {2.0, 1.2} },
      { "gauss_default", "cubic1", 1, 1, {-30}, {40}, {-50}, {60}, {-50}, {60},
        AST__GAUSS, 0, 0, 20, 0.0, {0.0, 3.0} },
      { "somb_unit", "unit2", 2, 2, {-3,2}, {16,16}, {-3,2}, {16,16}, {-3,2}, {16,16},
        AST__SOMB, AST__USEBAD, 1, 50, 0.0, {2.0, 2.0} },
      { "sombcos_win3", "win3", 3, 3, {-2,0,1}, {6,7,5}, {-3,-1,0}, {9,8,9}, {-3,-1,0}, {9,8,9},
        AST__SOMBCOS, AST__USEBAD, 1, 50, 0.1, {2.0, 2.0} },
      { "sombcos_unit", "unit2", 2, 2, {-3,2}, {16,16}, {-3,2}, {16,16}, {-3,2}, {16,16},
        AST__SOMBCOS, 0, 1, 50, 0.0, {2.0, 2.0} },
      { "sinc_nonlin3", "nonlin3", 3, 3, {-2,0,1}, {6,7,5}, {-3,-1,0}, {9,8,9}, {-3,-1,0}, {9,8,9},
        AST__SINC, 0, 1, 50, 0.0, {1.0, 0.0} },
      { "blockave_cubic", "cubic1", 1, 1, {-30}, {40}, {-50}, {60}, {-50}, {60},
        AST__BLOCKAVE, AST__USEBAD, 1, 20, 0.001, {1.0, 0.0} },
      { "blockave_zoom", "zoom2", 2, 2, {-3,2}, {16,16}, {-10,-5}, {30,30}, {-10,-5}, {30,30},
        AST__BLOCKAVE, AST__USEBAD, 1, 50, 0.1, {2.0, 0.0} },
      { "blockave_novarbad", "shift_half", 2, 2, {-3,2}, {16,16}, {-5,0}, {18,18}, {-5,0}, {18,18},
        AST__BLOCKAVE, 0, 1, 50, 0.1, {1.0, 0.0} },
      { "blockave_win3", "win3", 3, 3, {-2,0,1}, {6,7,5}, {-3,-1,0}, {9,8,9}, {-3,-1,0}, {9,8,9},
        AST__BLOCKAVE, 0, 1, 50, 0.1, {1.0, 0.0} },
      { "blockave_empty", "unit2", 2, 2, {-3,2}, {16,16}, {-3,2}, {16,16}, {-3,2}, {16,16},
        AST__BLOCKAVE, 0, 0, 50, 0.0, {0.0, 0.0} },
      { "ukern1_nonlin", "nonlin2", 2, 2, {-3,2}, {16,16}, {-6,-2}, {20,22}, {-6,-2}, {20,22},
        AST__UKERN1, AST__USEBAD, 1, 50, 0.0, {2.0, 0.75} },
      { "ukern1_win3", "win3", 3, 3, {-2,0,1}, {6,7,5}, {-3,-1,0}, {9,8,9}, {-3,-1,0}, {9,8,9},
        AST__UKERN1, 0, 1, 50, 0.1, {0.0, 1.0} },
      { "uinterp_zoom", "zoom2", 2, 2, {-3,2}, {16,16}, {-10,-5}, {30,30}, {-10,-5}, {30,30},
        AST__UINTERP, 0, 1, 50, 0.1, {1.3, 0.0} },
      { "conserve_zoom", "zoom2", 2, 2, {-3,2}, {16,16}, {-10,-5}, {30,30}, {-10,-5}, {30,30},
        AST__LINEAR, AST__USEBAD | AST__CONSERVEFLUX, 1, 50, 0.1, {0.0, 0.0} },
      { "conserve_perm", "perm", 2, 2, {-3,2}, {16,16}, {0,-5}, {20,20}, {0,-5}, {20,20},
        AST__SINC, AST__CONSERVEFLUX, 1, 50, 0.1, {2.0, 0.0} },
      { "conserve_win3", "win3", 3, 3, {-2,0,1}, {6,7,5}, {-3,-1,0}, {9,8,9}, {-3,-1,0}, {9,8,9},
        AST__BLOCKAVE, AST__USEBAD | AST__CONSERVEFLUX, 1, 50, 0.1, {1.0, 0.0} },
      { "conserve_cubic", "cubic1", 1, 1, {-30}, {40}, {-50}, {60}, {-50}, {60},
        AST__NEAREST, AST__CONSERVEFLUX, 0, 20, 0.001, {0.0, 0.0} },
      { "conserve_zoom1", "zoom1", 1, 1, {-30}, {40}, {-50}, {60}, {-50}, {60},
        AST__GAUSS, AST__CONSERVEFLUX, 1, 200, 0.5, {2.0, 1.0} },
   };
   astWatch( &status );
   printf( "// Generated by tools/oracle_fixtures/resample_probe.c. Do not edit.\n\n" );
   printf( "/// `(case, Mapping, type, ndim_in, lbnd_in, ubnd_in, lbnd_out, ubnd_out,\n" );
   printf( "/// region lbnd, region ubnd, interp, use_bad, no_bad, use_var, conserve_flux,\n" );
   printf( "/// params, tol, maxpix, bad pixels, status, FNV-1a hash, every 211th output\n" );
   printf( "/// value)`.\n" );
   printf( "pub type Case = (&'static str, &'static str, &'static str, usize,\n" );
   printf( "    &'static [i64], &'static [i64], &'static [i64], &'static [i64],\n" );
   printf( "    &'static [i64], &'static [i64], &'static str, bool, bool, bool, bool,\n" );
   printf( "    [f64; 2], f64, usize, usize, i32, u64, &'static [f64]);\n" );
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
      AstMapping *nl = mapping( "nonlin2" ), *p23;
      int codes[ 15 ];
      int inperm[ 2 ] = { 1, 2 }, outperm[ 3 ] = { 1, 2, -1 };
      AstDim l3[ 3 ] = { 1, 1, 1 }, u3[ 3 ] = { 10, 10, 1 };
      double kpar[ 2 ] = { 2.0, -1.0 }, upar[ 2 ] = { -1.0, 0.0 };
      double shalf[ 2 ] = { 0.5, 0.5 };
      AstMapping *sh = (AstMapping *) astShiftMap( 2, shalf, " " );
      p23 = (AstMapping *) astPermMap( 2, inperm, 3, outperm, shalf, " " );
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
      TRY( 8, astResample8D( u, 2, l2, u2, in, NULL, AST__LINEAR, NULL, NULL, AST__CONSERVEFLUX, 0.0, 50, -1.0, 2, l2, u2, l2, u2, out, NULL ) )
      TRY( 9, astResample8D( p23, 2, l2, u2, in, NULL, AST__LINEAR, NULL, NULL, AST__CONSERVEFLUX, 0.1, 50, -1.0, 3, l3, u3, l3, u3, out, NULL ) )
      TRY( 10, astResample8D( nl, 2, l2, u2, in, NULL, AST__LINEAR, NULL, NULL, AST__CONSERVEFLUX, 1.0e-12, 50, -1.0, 2, l2, u2, l2, u2, out, NULL ) )
      TRY( 11, astResample8D( u, 2, l2, u2, in, NULL, AST__LINEAR, NULL, NULL, AST__CONSERVEFLUX | AST__NOBAD, 0.1, 50, -1.0, 2, l2, u2, l2, u2, out, NULL ) )
      TRY( 12, astResample8D( sh, 2, l2, u2, in, NULL, AST__UKERN1, (void (*)( void )) tri_kernel, kpar, 0, 0.0, 50, -1.0, 2, l2, u2, l2, u2, out, NULL ) )
      TRY( 13, astResample8D( u, 2, l2, u2, in, NULL, AST__UINTERP, (void (*)( void )) uinterp_D, upar, 0, 0.0, 50, -1.0, 2, l2, u2, l2, u2, out, NULL ) )
      TRY( 14, astResample8D( u, 2, l2, u2, in, NULL, AST__UKERN1, (void (*)( void )) tri_kernel, kpar, 0, 0.0, 50, -1.0, 2, l2, u2, l2, u2, out, NULL ) )
      printf( "/// The status for: too few input dimensions, too many output\n" );
      printf( "/// dimensions, an input lower bound above its upper bound, the same for\n" );
      printf( "/// the output grid, a region below and above the output grid, a negative\n" );
      printf( "/// tolerance, a Mapping with no inverse; flux conservation with zero\n" );
      printf( "/// tolerance, with a Mapping from 2 to 3 axes, with a Mapping too\n" );
      printf( "/// non-linear to fit, and with AST__NOBAD; a user kernel that fails at\n" );
      printf( "/// offset 1.5 used with a half-pixel shift, a user interpolator that\n" );
      printf( "/// fails, and the same kernel used where no offset is 1.5.\n" );
      printf( "pub const ERRORS: [i32; 15] = [" );
      for( i = 0; i < 15; i++ ) printf( "%s%d", i ? ", " : "", codes[ i ] );
      printf( "];\n" );
   }
   return 0;
}
