/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* tran_grid_probe -- capture astTranGrid and astQuadApprox (mapping.c).
 *
 * For astTranGrid: grids transformed through a ZoomMap, the FITS-WCS
 * pixel-to-sky FrameSet of testtrangrid.c (with an NAXIS card added, which
 * leaves C's results unchanged), its 3-D CmpMap, a 1-D cubic
 * MathMap, a 2-in 3-out MatrixMap and a 2-in 1-out PolyMap, forward and
 * inverse, with and without a linear-fit tolerance, with a small and a
 * large initial scale size, and with grids large enough to be simplified
 * and transformed in blocks. Each case records an FNV-1a hash of the bit
 * patterns of every output (position by position, axes in order) and every
 * 97th position's values. Also the status for invalid arguments.
 *
 * For astQuadApprox: the coefficients, RMS and returned value for a
 * quadratic PolyMap, the FrameSet, the MatrixMap, a MathMap bad over part
 * of the box, a MathMap whose second output is bad everywhere, a box of
 * zero width, and a Mapping with one input.
 *
 * Prints Rust source for crates/starlink-ast/tests/tran_grid_oracle/cases.rs:
 *
 *   AST_REPLACE_NAN=1 ./tran_grid_probe > ../../crates/starlink-ast/tests/tran_grid_oracle/cases.rs
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"

/* Print a double as a Rust f64 literal. */
static void dbl( double v ) {
   char buf[ 40 ];
   snprintf( buf, sizeof( buf ), "%.17g", v );
   printf( "%s%s", buf, strpbrk( buf, ".en" ) ? "" : ".0" );
}

static AstMapping *zoom2( void ) {
   return (AstMapping *) astZoomMap( 2, 2.0, " " );
}

static AstMapping *tan( void ) {
   const char *cards[] = {
      "CTYPE1  = 'RA---TAN'", "CTYPE2  = 'DEC--TAN'", "CRPIX1  = 20",
      "CRPIX2  = 20", "CRVAL1  = 0.0", "CRVAL2  = 0.0", "CROTA1  = 30.0",
      "CDELT1  = -0.001", "CDELT2  = 0.001", "NAXIS   = 2", NULL };
   AstFitsChan *fc = astFitsChan( NULL, NULL, " " );
   int i;
   for( i = 0; cards[ i ]; i++ ) astPutFits( fc, cards[ i ], 0 );
   astClear( fc, "Card" );
   return (AstMapping *) astRead( fc );
}

static AstMapping *cmp3( void ) {
   double sh = -1.0, mat[ 4 ] = { 1.0, 2.0, -2.0, 3.0 }, zero = 0.0;
   int perm[ 3 ] = { 3, 2, 1 };
   AstMapping *c1, *c2, *c3, *c4;
   c1 = (AstMapping *) astShiftMap( 1, &sh, " " );
   c2 = (AstMapping *) astMatrixMap( 2, 2, 0, mat, " " );
   c3 = (AstMapping *) astCmpMap( c1, c2, 0, " " );
   c1 = (AstMapping *) astPermMap( 3, perm, 3, perm, &zero, " " );
   c2 = (AstMapping *) astZoomMap( 3, 0.25, " " );
   astInvert( c2 );
   c4 = (AstMapping *) astCmpMap( c1, c2, 1, " " );
   astInvert( c4 );
   return (AstMapping *) astCmpMap( c3, c4, 1, " " );
}

static AstMapping *cubic( void ) {
   const char *fwd = "y=x**3";
   const char *inv = "x=sign((abs(y)**(1/3)),y)";
   return (AstMapping *) astMathMap( 1, 1, 1, &fwd, 1, &inv, " " );
}

static AstMapping *cubic_fwd_only( void ) {
   const char *fwd = "y=x**3";
   const char *inv = "x";
   return (AstMapping *) astMathMap( 1, 1, 1, &fwd, 1, &inv, " " );
}

static AstMapping *matrix23( void ) {
   double mat[ 6 ] = { 1.0, 2.0, 3.0, -4.0, 0.5, 6.0 };
   return (AstMapping *) astMatrixMap( 2, 3, 0, mat, " " );
}

static AstMapping *poly( void ) {
   double coeffs[] = { 2.0, 1, 0, 0, 3.0, 1, 1, 0, 4.0, 1, 0, 1,
                       5.0, 1, 1, 1, 6.0, 1, 2, 0, 7.0, 1, 0, 2 };
   return (AstMapping *) astPolyMap( 2, 1, 6, coeffs, 0, NULL, " " );
}

static AstMapping *sqrt_part( void ) {
   const char *fwd = "o=sqrt(x)*y";
   const char *inv[] = { "x", "y" };
   return (AstMapping *) astMathMap( 2, 1, 1, &fwd, 2, inv, " " );
}

static AstMapping *second_bad( void ) {
   const char *fwd[] = { "o1=x+2*y*y", "o2=sqrt(-1-x*x)" };
   const char *inv[] = { "x", "y" };
   return (AstMapping *) astMathMap( 2, 2, 2, fwd, 2, inv, " " );
}

static void grid( const char *name, const char *map, AstMapping *m, int nin,
                  const AstDim *lbnd, const AstDim *ubnd, double tol,
                  int maxpix, int forward ) {
   AstDim npoint = 1, i;
   int nout, c, idim;
   double *out;
   uint64_t hash = 14695981039346656037ULL, bits;

   for( idim = 0; idim < nin; idim++ ) npoint *= ubnd[ idim ] - lbnd[ idim ] + 1;
   nout = astGetI( m, forward ? "Nout" : "Nin" );
   out = malloc( sizeof( double ) * npoint * nout );
   astTranGrid8( m, nin, lbnd, ubnd, tol, maxpix, forward, nout, npoint, out );
   for( i = 0; i < npoint; i++ ) {
      for( c = 0; c < nout; c++ ) {
         memcpy( &bits, out + c * npoint + i, 8 );
         hash = ( hash ^ bits ) * 1099511628211ULL;
      }
   }
   printf( "    (\"%s\", \"%s\", &[", name, map );
   for( idim = 0; idim < nin; idim++ ) printf( "%s%lld", idim ? ", " : "", (long long) lbnd[ idim ] );
   printf( "], &[" );
   for( idim = 0; idim < nin; idim++ ) printf( "%s%lld", idim ? ", " : "", (long long) ubnd[ idim ] );
   printf( "], " );
   dbl( tol );
   printf( ", %d, %s, %llu, &[", maxpix, forward ? "true" : "false",
           (unsigned long long) hash );
   for( i = 0; i < npoint; i += 97 ) {
      for( c = 0; c < nout; c++ ) {
         printf( "%s", ( i || c ) ? ", " : "" );
         dbl( out[ c * npoint + i ] );
      }
   }
   printf( "]),\n" );
   free( out );
}

static void quad( const char *name, const char *map, AstMapping *m,
                  double l0, double l1, double u0, double u1, int nx, int ny ) {
   double lbnd[ 2 ] = { l0, l1 }, ubnd[ 2 ] = { u0, u1 }, fit[ 18 ], rms;
   int ok, i, nout = astGetI( m, "Nout" );
   for( i = 0; i < 18; i++ ) fit[ i ] = AST__BAD;
   ok = astQuadApprox( m, lbnd, ubnd, nx, ny, fit, &rms );
   printf( "    (\"%s\", \"%s\", [", name, map );
   dbl( l0 );
   printf( ", " );
   dbl( l1 );
   printf( "], [" );
   dbl( u0 );
   printf( ", " );
   dbl( u1 );
   printf( "], %d, %d, %s, ", nx, ny, ok ? "true" : "false" );
   dbl( rms );
   printf( ", &[" );
   for( i = 0; i < 6 * nout; i++ ) {
      printf( "%s", i ? ", " : "" );
      dbl( fit[ i ] );
   }
   printf( "]),\n" );
}

int main( void ) {
   int status = 0;
   AstMapping *z = zoom2(), *t = tan(), *c3 = cmp3(), *cu = cubic(),
              *m23 = matrix23(), *p = poly();
   double out[ 100 ];

   astWatch( &status );
   printf( "// Generated by tools/oracle_fixtures/tran_grid_probe.c. Do not edit.\n\n" );

   printf( "/// `(case, Mapping, lbnd, ubnd, tol, maxpix, forward, FNV-1a hash of the\n" );
   printf( "/// output bits, every 97th position's outputs)`.\n" );
   printf( "pub type Grid = (&'static str, &'static str, &'static [i64], &'static [i64], f64,\n" );
   printf( "    usize, bool, u64, &'static [f64]);\n" );
   printf( "pub const GRIDS: &[Grid] = &[\n" );
   {
      AstDim l2[] = { -6, 0 }, u2[] = { 15, 30 };
      AstDim lb[] = { -50, -40 }, ub[] = { 60, 50 };
      AstDim lt[] = { 1, 1 }, ut[] = { 100, 80 };
      AstDim l3[] = { -6, 0, 10 }, u3[] = { 15, 30, 40 };
      AstDim l1[] = { -100 }, u1[] = { 200 };
      AstDim li[] = { -1000 }, ui[] = { 1000 };
      AstDim lw[] = { -3000 }, uw[] = { 3000 };
      AstDim lm[] = { 1, 1 }, um[] = { 70, 60 };
      AstDim lp[] = { -20, -20 }, up[] = { 20, 20 };
      AstDim lzi[] = { -12, 0 }, uzi[] = { 30, 60 };

      grid( "zoom_small", "zoom2", z, 2, l2, u2, 0.5, 100, 1 );
      grid( "zoom_small_exact", "zoom2", z, 2, l2, u2, 0.0, 100, 1 );
      grid( "zoom_small_divided", "zoom2", z, 2, l2, u2, 0.5, 4, 1 );
      grid( "zoom_big", "zoom2", z, 2, lb, ub, 0.5, 100, 1 );
      grid( "zoom_big_exact", "zoom2", z, 2, lb, ub, 0.0, 100, 1 );
      grid( "zoom_inverse", "zoom2", z, 2, lzi, uzi, 0.5, 100, 0 );
      grid( "tan_small", "tan", t, 2, l2, u2, 1.0e-4, 100, 1 );
      grid( "tan_small_exact", "tan", t, 2, l2, u2, 0.0, 100, 1 );
      grid( "tan_small_divided", "tan", t, 2, l2, u2, 1.0e-4, 4, 1 );
      grid( "tan_big", "tan", t, 2, lt, ut, 1.0e-4, 50, 1 );
      grid( "tan_big_loose", "tan", t, 2, lt, ut, 1.0, 1000, 1 );
      grid( "cmp3", "cmp3", c3, 3, l3, u3, 0.5, 100, 1 );
      grid( "cmp3_exact", "cmp3", c3, 3, l3, u3, 0.0, 100, 1 );
      grid( "cmp3_divided", "cmp3", c3, 3, l3, u3, 0.5, 4, 1 );
      grid( "cmp3_inverse", "cmp3", c3, 3, l3, u3, 0.5, 100, 0 );
      grid( "cubic", "cubic", cu, 1, l1, u1, 1.0e-4, 100, 1 );
      grid( "cubic_inverse", "cubic", cu, 1, li, ui, 1.0e-4, 100, 0 );
      grid( "cubic_wide_loose", "cubic", cu, 1, lw, uw, 1.0e6, 100000, 1 );
      grid( "matrix23", "matrix23", m23, 2, lm, um, 0.5, 100, 1 );
      grid( "matrix23_exact", "matrix23", m23, 2, lm, um, 0.0, 100, 1 );
      grid( "poly", "poly", p, 2, lp, up, 0.01, 10, 1 );
      grid( "poly_loose", "poly", p, 2, lp, up, 100.0, 100, 1 );
   }
   printf( "];\n\n" );

   printf( "/// The status of astTranGrid with: a lower bound above its upper bound,\n" );
   printf( "/// a negative tolerance, a transform that is not defined, and a\n" );
   printf( "/// Mapping with the wrong number of inputs.\n" );
   {
      AstDim lo[] = { 5, 0 }, hi[] = { 3, 4 }, l1[] = { 0 }, u1[] = { 4 };
      int codes[ 4 ];
      astTranGrid8( z, 2, lo, hi, 0.5, 100, 1, 2, 100, out );
      codes[ 0 ] = status;
      astClearStatus;
      astTranGrid8( z, 2, hi, hi, -1.0, 100, 1, 2, 100, out );
      codes[ 1 ] = status;
      astClearStatus;
      astTranGrid8( cubic_fwd_only(), 1, l1, u1, 0.5, 100, 0, 1, 100, out );
      codes[ 2 ] = status;
      astClearStatus;
      astTranGrid8( z, 1, l1, u1, 0.5, 100, 1, 2, 100, out );
      codes[ 3 ] = status;
      astClearStatus;
      printf( "pub const GRID_ERRORS: [i32; 4] = [%d, %d, %d, %d];\n\n", codes[ 0 ],
              codes[ 1 ], codes[ 2 ], codes[ 3 ] );
   }

   printf( "/// `(case, Mapping, lbnd, ubnd, nx, ny, returned value, rms, coefficients)`.\n" );
   printf( "pub type Quad = (&'static str, &'static str, [f64; 2], [f64; 2], usize, usize,\n" );
   printf( "    bool, f64, &'static [f64]);\n" );
   printf( "pub const QUADS: &[Quad] = &[\n" );
   quad( "poly", "poly", p, -1.0, -1.0, 1.0, 1.0, 50, 50 );
   quad( "poly_coarse", "poly", p, -3.0, 2.0, 5.0, 9.0, 1, 4 );
   quad( "tan", "tan", t, -6.0, 0.0, 15.0, 30.0, 10, 7 );
   quad( "matrix23", "matrix23", m23, 0.0, 0.0, 5.0, 5.0, 2, 2 );
   quad( "sqrt_part", "sqrt_part", sqrt_part(), -1.0, -1.0, 1.0, 1.0, 11, 11 );
   quad( "second_bad", "second_bad", second_bad(), -1.0, -1.0, 1.0, 1.0, 9, 9 );
   quad( "zero_width", "zoom2", z, 2.0, -1.0, 2.0, 1.0, 5, 5 );
   printf( "];\n\n" );

   {
      double l[ 2 ] = { 0.0, 0.0 }, u[ 2 ] = { 1.0, 1.0 }, fit[ 6 ], rms;
      astQuadApprox( cu, l, u, 5, 5, fit, &rms );
      printf( "/// The status of astQuadApprox for a Mapping with one input.\n" );
      printf( "pub const QUAD_ONE_INPUT: i32 = %d;\n", status );
      astClearStatus;
   }
   return 0;
}
