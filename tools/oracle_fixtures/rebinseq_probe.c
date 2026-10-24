/* rebinseq_probe -- capture sequences of astRebinSeq<X> calls (mapping.c)
 * for every data type it exists for (D, F, I, B, UB): the first call of
 * each sequence sets AST__REBININIT and the last AST__REBINEND, and some
 * sequences end with a call that has no input and only normalises. The
 * cases cover nearest, linear and kernel spreading in 1-D, 2-D and 3-D,
 * output variances from input variances (AST__USEVAR) and from the spread
 * of input values (AST__GENVAR, with and without AST__DISVAR), inverse
 * variance weights (AST__VARWGT), a constant weight (AST__PARWGT), flux
 * conservation, unnormalised sums (AST__NONORM, with and without weights),
 * and a weight limit. Each case records every call's status, the count of
 * input pixels used, an FNV-1a hash of the bit patterns of every output
 * value, variance and weight converted to double, and every 61st output
 * value. Also, for invalid arguments, the status and what C left in the
 * output, weights and count.
 *
 * Input pixel k of call c holds, with j = k + 13c, (7j mod 23) - 11 for a
 * signed integer type, (7j mod 23) for an unsigned one, and
 * (7j mod 97)*0.37 - 10 for a floating one; its variance (3j mod 11) + 1
 * and (3j mod 11)*0.5 + 0.25. With AST__USEBAD, pixels with j mod 17 = 3
 * are bad, as are variances with j mod 19 = 5. The bad value is 120.
 * Output arrays start as 77, weights as 5 and the count as 9.
 *
 * Prints Rust source for crates/starlink-ast/tests/rebinseq_oracle/cases.rs:
 *
 *   AST_REPLACE_NAN=1 ./rebinseq_probe > ../../crates/starlink-ast/tests/rebinseq_oracle/cases.rs
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
   if( !strcmp( name, "shift_whole" ) ) {
      double sh[ 2 ] = { 1.0, -2.0 };
      return (AstMapping *) astShiftMap( 2, sh, " " );
   }
   if( !strcmp( name, "far" ) ) {
      double sh[ 2 ] = { 1000.0, 1000.0 };
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
   const char *name;
   int ncall;
   const char *maps[ 3 ];
   int end_only;              /* Add a last call with no input? */
   int ndim;
   AstDim lbnd_in[ 3 ], ubnd_in[ 3 ], lbnd_out[ 3 ], ubnd_out[ 3 ], lbnd[ 3 ], ubnd[ 3 ];
   int spread, flags, in_var, out_var, weights, maxpix;
   double tol, wlim;
   double params[ 3 ];
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

#define FLAGS(f) ( (f) & AST__USEBAD ) ? "true" : "false", ( (f) & AST__USEVAR ) ? "true" : "false", \
   ( (f) & AST__GENVAR ) ? "true" : "false", ( (f) & AST__VARWGT ) ? "true" : "false", \
   ( (f) & AST__DISVAR ) ? "true" : "false", ( (f) & AST__NONORM ) ? "true" : "false", \
   ( (f) & AST__PARWGT ) ? "true" : "false", ( (f) & AST__CONSERVEFLUX ) ? "true" : "false"

static uint64_t fnv( uint64_t hash, double d ) {
   uint64_t bits;
   memcpy( &bits, &d, 8 );
   return ( hash ^ bits ) * 1099511628211ULL;
}

#define RUN(X,Xtype,Floating,Signed) \
static void run_##X( const Case *c ) { \
   AstDim nin = npix( c->ndim, c->lbnd_in, c->ubnd_in ); \
   AstDim nout = npix( c->ndim, c->lbnd_out, c->ubnd_out ); \
   Xtype *in = malloc( sizeof( Xtype ) * nin ), *in_var = malloc( sizeof( Xtype ) * nin ); \
   Xtype *out = malloc( sizeof( Xtype ) * nout ), *out_var = malloc( sizeof( Xtype ) * nout ); \
   double *weights = malloc( sizeof( double ) * 2 * nout ); \
   int64_t nused = 9; \
   Xtype badval = (Xtype) 120; \
   AstDim k, j; \
   uint64_t hash = 14695981039346656037ULL; \
   int status[ 4 ], call, ncall = c->ncall + c->end_only, flags; \
   int nw = ( c->flags & AST__GENVAR ) ? 2 : 1; \
   for( k = 0; k < nout; k++ ) out[ k ] = out_var[ k ] = (Xtype) 77; \
   for( k = 0; k < 2 * nout; k++ ) weights[ k ] = 5.0; \
   for( call = 0; call < ncall; call++ ) { \
      flags = c->flags; \
      if( call == 0 ) flags |= AST__REBININIT; \
      if( call == ncall - 1 ) flags |= AST__REBINEND; \
      for( k = 0; k < nin; k++ ) { \
         j = k + 13 * call; \
         if( Floating ) { \
            in[ k ] = (Xtype) ( ( ( 7 * j ) % 97 ) * 0.37 - 10.0 ); \
            in_var[ k ] = (Xtype) ( ( ( 3 * j ) % 11 ) * 0.5 + 0.25 ); \
         } else { \
            in[ k ] = (Xtype) ( ( ( 7 * j ) % 23 ) - ( Signed ? 11 : 0 ) ); \
            in_var[ k ] = (Xtype) ( ( ( 3 * j ) % 11 ) + 1 ); \
         } \
         if( ( c->flags & AST__USEBAD ) && j % 17 == 3 ) in[ k ] = badval; \
         if( ( c->flags & AST__USEBAD ) && j % 19 == 5 ) in_var[ k ] = badval; \
      } \
      AstMapping *m = mapping( c->maps[ call < c->ncall ? call : 0 ] ); \
      astRebinSeq8##X( m, c->wlim, c->ndim, c->lbnd_in, c->ubnd_in, \
                       call < c->ncall ? in : NULL, c->in_var ? in_var : NULL, \
                       c->spread, c->params, flags, c->tol, c->maxpix, badval, \
                       c->ndim, c->lbnd_out, c->ubnd_out, c->lbnd, c->ubnd, out, \
                       c->out_var ? out_var : NULL, c->weights ? weights : NULL, \
                       c->weights ? &nused : NULL ); \
      status[ call ] = astStatus; \
      astClearStatus; \
      m = astAnnul( m ); \
   } \
   for( k = 0; k < nout; k++ ) { \
      hash = fnv( hash, (double) out[ k ] ); \
      if( c->out_var ) hash = fnv( hash, (double) out_var[ k ] ); \
   } \
   if( c->weights ) for( k = 0; k < nw * nout; k++ ) hash = fnv( hash, weights[ k ] ); \
   printf( "    (\"%s\", \"" #X "\", &[", c->name ); \
   for( call = 0; call < c->ncall; call++ ) printf( "%s\"%s\"", call ? ", " : "", c->maps[ call ] ); \
   printf( "], %s, %d, ", c->end_only ? "true" : "false", c->ndim ); \
   dims( c->ndim, c->lbnd_in ); printf( ", " ); \
   dims( c->ndim, c->ubnd_in ); printf( ", " ); \
   dims( c->ndim, c->lbnd_out ); printf( ", " ); \
   dims( c->ndim, c->ubnd_out ); printf( ", " ); \
   dims( c->ndim, c->lbnd ); printf( ", " ); \
   dims( c->ndim, c->ubnd ); \
   printf( ", \"%s\", [%s, %s, %s, %s, %s, %s, %s, %s], %s, %s, %s, [", spread_name( c->spread ), \
           FLAGS( c->flags ), c->in_var ? "true" : "false", c->out_var ? "true" : "false", \
           c->weights ? "true" : "false" ); \
   dbl( c->params[ 0 ] ); printf( ", " ); dbl( c->params[ 1 ] ); printf( ", " ); \
   dbl( c->params[ 2 ] ); printf( "], " ); \
   dbl( c->tol ); printf( ", %d, ", c->maxpix ); dbl( c->wlim ); printf( ", &[" ); \
   for( call = 0; call < ncall; call++ ) printf( "%s%d", call ? ", " : "", status[ call ] ); \
   printf( "], %lld, %llu, &[", (long long) nused, (unsigned long long) hash ); \
   for( k = 0; k < nout; k += 61 ) { \
      printf( "%s", k ? ", " : "" ); \
      dbl( (double) out[ k ] ); \
   } \
   printf( "]),\n" ); \
   free( in ); free( in_var ); free( out ); free( out_var ); free( weights ); \
}

RUN(D,double,1,1)
RUN(F,float,1,1)
RUN(I,int,0,1)
RUN(B,signed char,0,1)
RUN(UB,unsigned char,0,0)

#define C2(...) {-3,2}, {16,16}, __VA_ARGS__, {-3,2}, {16,16}

int main( void ) {
   int status = 0, i;
   static const Case cases[] = {
      { "linear_usevar", 2, { "unit2", "shift_half" }, 0, 2, C2( {-5,0}, {18,18} ),
        AST__LINEAR, AST__USEBAD | AST__USEVAR, 1, 1, 1, 50, 0.1, 0.0 },
      { "nearest_genvar", 3, { "unit2", "shift_half", "shift_whole" }, 0, 2, C2( {-5,0}, {18,18} ),
        AST__NEAREST, AST__GENVAR, 0, 1, 1, 50, 0.1, 0.0 },
      { "linear_genvar_disvar", 2, { "shift_half", "zoom2" }, 1, 2, C2( {-5,0}, {30,30} ),
        AST__LINEAR, AST__USEBAD | AST__GENVAR | AST__DISVAR, 1, 1, 1, 50, 0.1, 0.0 },
      { "linear_varwgt", 2, { "unit2", "shift_half" }, 0, 2, C2( {-5,0}, {18,18} ),
        AST__LINEAR, AST__USEBAD | AST__VARWGT | AST__USEVAR, 1, 1, 1, 50, 0.0, 0.0 },
      { "nearest_varwgt_genvar", 2, { "unit2", "shrink2" }, 0, 2, C2( {-5,-1}, {18,18} ),
        AST__NEAREST, AST__VARWGT | AST__GENVAR, 1, 1, 1, 50, 0.1, 0.0 },
      { "sinc_parwgt", 2, { "shift_half", "unit2" }, 0, 2, C2( {-5,0}, {18,18} ),
        AST__SINC, AST__PARWGT | AST__USEVAR, 1, 1, 1, 50, 0.1, 0.0, {0.7, 2.0, 0.0} },
      { "sincgauss_genvar", 2, { "zoom2", "shift_half" }, 0, 2, C2( {-5,0}, {30,30} ),
        AST__SINCGAUSS, AST__USEBAD | AST__GENVAR, 0, 1, 1, 50, 0.1, 0.0, {0.0, 1.5, 0.0} },
      { "gauss_varwgt", 2, { "unit2", "shift_half" }, 0, 2, C2( {-5,0}, {18,18} ),
        AST__GAUSS, AST__VARWGT | AST__USEVAR, 1, 1, 1, 50, 0.0, 0.0, {2.0, 1.2, 0.0} },
      { "conserve_zoom", 2, { "zoom2", "shrink2" }, 0, 2, C2( {-10,-5}, {30,30} ),
        AST__LINEAR, AST__CONSERVEFLUX | AST__USEVAR, 1, 1, 1, 50, 0.1, 0.0 },
      { "conserve_nonlin_cnflx", 2, { "unit2", "nonlin2" }, 0, 2, C2( {-6,-2}, {20,22} ),
        AST__NEAREST, AST__CONSERVEFLUX, 0, 0, 1, 8, 1.0e-9, 0.0 },
      { "nonorm", 2, { "unit2", "shift_half" }, 0, 2, C2( {-5,0}, {18,18} ),
        AST__LINEAR, AST__NONORM | AST__USEVAR, 1, 1, 1, 50, 0.1, 0.0 },
      { "nonorm_no_weights", 2, { "unit2", "shift_half" }, 0, 2, C2( {-5,0}, {18,18} ),
        AST__LINEAR, AST__NONORM, 0, 0, 0, 50, 0.1, 0.0 },
      { "wlim", 2, { "unit2", "zoom2" }, 1, 2, C2( {-5,0}, {30,30} ),
        AST__NEAREST, AST__USEVAR, 1, 1, 1, 50, 0.1, 0.6 },
      { "wlim_shrink", 2, { "shrink2", "shift_half" }, 0, 2, C2( {-5,-1}, {18,18} ),
        AST__LINEAR, AST__USEBAD, 0, 0, 1, 50, 0.1, 1.0 },
      { "cubic_genvar", 2, { "cubic1", "cubic1" }, 0, 1, {-30}, {40}, {-50}, {80}, {-30}, {40},
        AST__LINEAR, AST__GENVAR, 0, 1, 1, 20, 0.001, 0.0 },
      { "win3_varwgt", 2, { "win3", "nonlin3" }, 0, 3, {-2,0,1}, {6,7,5}, {-3,-1,0}, {9,8,9},
        {-2,0,1}, {6,7,5}, AST__LINEAR, AST__USEBAD | AST__VARWGT | AST__GENVAR, 1, 1, 1, 50, 0.1, 0.0 },
      { "sombcos_win3", 1, { "win3" }, 1, 3, {-2,0,1}, {6,7,5}, {-3,-1,0}, {9,8,9},
        {-2,0,1}, {6,7,5}, AST__SOMBCOS, AST__USEVAR, 1, 1, 1, 50, 0.1, 0.0, {2.0, 2.0, 0.0} },
      { "nothing_used", 1, { "far" }, 0, 2, C2( {-5,0}, {18,18} ),
        AST__LINEAR, 0, 0, 0, 1, 50, 0.0, 0.0 },
   };
   astWatch( &status );
   printf( "// Generated by tools/oracle_fixtures/rebinseq_probe.c. Do not edit.\n\n" );
   printf( "/// `(case, type, Mapping of each call, a last call without input, ndim,\n" );
   printf( "/// lbnd_in, ubnd_in, lbnd_out, ubnd_out, region lbnd, region ubnd, spread,\n" );
   printf( "/// [use_bad, use_var, gen_var, var_wgt, dis_var, no_norm, par_wgt,\n" );
   printf( "/// conserve_flux], input variances given, output variances given, weights\n" );
   printf( "/// and count given, params, tol, maxpix, wlim, status of each call, count,\n" );
   printf( "/// FNV-1a hash, every 61st output value)`.\n" );
   printf( "pub type Case = (&'static str, &'static str, &'static [&'static str], bool, usize,\n" );
   printf( "    &'static [i64], &'static [i64], &'static [i64], &'static [i64],\n" );
   printf( "    &'static [i64], &'static [i64], &'static str, [bool; 8], bool, bool, bool,\n" );
   printf( "    [f64; 3], f64, usize, f64, &'static [i32], i64, u64, &'static [f64]);\n" );
   printf( "pub const CASES: &[Case] = &[\n" );
   for( i = 0; i < (int) ( sizeof( cases ) / sizeof( cases[ 0 ] ) ); i++ ) {
      run_D( cases + i ); run_F( cases + i ); run_I( cases + i );
      run_B( cases + i ); run_UB( cases + i );
   }
   printf( "];\n\n" );

   /* Errors, all for double data, a single call with both REBININIT and
      REBINEND unless stated, the output starting as 7, the weights as 5 and
      the count as 9. */
   {
      double in[ 100 ], in_var[ 100 ], out[ 100 ], out_var[ 100 ], w[ 200 ];
      AstDim l2[ 2 ] = { 1, 1 }, u2[ 2 ] = { 10, 10 }, bad[ 2 ] = { 11, 1 };
      AstMapping *u = mapping( "unit2" );
      int64_t nused;
      int codes[ 12 ];
      double first[ 12 ][ 4 ];
      int64_t nu[ 12 ];
      const int IE = AST__REBININIT | AST__REBINEND;
      for( i = 0; i < 100; i++ ) { in[ i ] = i % 7; in_var[ i ] = 1.0; }
#define TRY(j, call) for( i = 0; i < 100; i++ ) out[ i ] = out_var[ i ] = 7.0; \
      for( i = 0; i < 200; i++ ) w[ i ] = 5.0; \
      nused = 9; call; codes[ j ] = astStatus; astClearStatus; \
      first[ j ][ 0 ] = out[ 0 ]; first[ j ][ 1 ] = out[ 1 ]; \
      first[ j ][ 2 ] = out_var[ 0 ]; first[ j ][ 3 ] = w[ 0 ]; nu[ j ] = nused;
#define SEQ(m, ndin, lin, var, flags, ov, wt, nup, lout) \
      astRebinSeq8D( m, 0.0, ndin, lin, u2, in, var, AST__LINEAR, NULL, flags, 0.0, 50, -1.0, \
                     2, lout, u2, l2, u2, out, ov, wt, nup )
      TRY( 0, SEQ( u, 2, l2, in_var, IE | AST__USEVAR | AST__GENVAR, out_var, w, &nused, l2 ) )
      TRY( 1, SEQ( u, 2, l2, NULL, IE | AST__USEVAR, out_var, w, &nused, l2 ) )
      TRY( 2, SEQ( u, 2, l2, NULL, IE | AST__VARWGT, out_var, w, &nused, l2 ) )
      TRY( 3, SEQ( u, 2, l2, in_var, IE | AST__USEVAR, NULL, w, &nused, l2 ) )
      TRY( 4, SEQ( u, 2, l2, in_var, IE | AST__GENVAR, NULL, w, &nused, l2 ) )
      TRY( 5, SEQ( u, 2, l2, in_var, IE | AST__NONORM | AST__GENVAR, out_var, w, &nused, l2 ) )
      TRY( 6, SEQ( u, 2, l2, in_var, IE, out_var, NULL, &nused, l2 ) )
      TRY( 7, SEQ( u, 2, l2, in_var, IE, out_var, w, NULL, l2 ) )
      TRY( 8, SEQ( u, 1, l2, in_var, IE, out_var, w, &nused, l2 ) )
      TRY( 9, SEQ( u, 1, l2, in_var, IE, out_var, NULL, &nused, l2 ) )
      TRY( 10, SEQ( u, 2, bad, in_var, IE | AST__USEVAR, out_var, w, &nused, l2 ) )
      TRY( 11, SEQ( u, 2, l2, in_var, AST__USEVAR, out_var, w, &nused, l2 ) )
      printf( "/// `(status, [out[0], out[1], out_var[0], weights[0]], count)` for one\n" );
      printf( "/// call with AST__REBININIT and AST__REBINEND and: AST__USEVAR with\n" );
      printf( "/// AST__GENVAR, AST__USEVAR and AST__VARWGT without input variances,\n" );
      printf( "/// AST__USEVAR and AST__GENVAR without output variances, AST__NONORM\n" );
      printf( "/// with AST__GENVAR, no weights, no count, too few input dimensions,\n" );
      printf( "/// the same with no weights, an input lower bound above its upper bound;\n" );
      printf( "/// and one valid call with neither flag. The output starts as 7, the\n" );
      printf( "/// weights as 5 and the count as 9.\n" );
      printf( "pub const ERRORS: [(i32, [f64; 4], i64); 12] = [\n" );
      for( i = 0; i < 12; i++ ) {
         printf( "    (%d, [", codes[ i ] );
         dbl( first[ i ][ 0 ] ); printf( ", " ); dbl( first[ i ][ 1 ] ); printf( ", " );
         dbl( first[ i ][ 2 ] ); printf( ", " ); dbl( first[ i ][ 3 ] );
         printf( "], %lld),\n", (long long) nu[ i ] );
      }
      printf( "];\n" );
   }
   return 0;
}
