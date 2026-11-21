/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* region_trace_probe -- capture what the protected astRegTrace and
 * astRegSplit return for Regions of every class that overrides them (region.c
 * RegTrace and RegSplit; box.c, circle.c, ellipse.c, interval.c, polygon.c
 * and moc.c RegTrace; cmpregion.c RegTrace, SetBreakInfo and RegSplit).
 *
 * For each Region: whether the class supports tracing (n = 0), and the
 * current-Frame positions at two lists of distances, one non-decreasing and
 * one not. For each CmpRegion split: the Native dump of each part.
 *
 * Prints Rust source for crates/starlink-ast/tests/region_trace_oracle/cases.rs:
 *
 *   ./region_trace_probe > ../../crates/starlink-ast/tests/region_trace_oracle/cases.rs
 *
 * Build from reference/starlink-ast/build (configured with
 * -DCMAKE_C_FLAGS=-ffp-contract=off):
 *
 *   cc ../../../tools/oracle_fixtures/region_trace_probe.c -I. -L. -last -lm \
 *      -Wl,-rpath,$PWD -o ../../../tools/oracle_fixtures/region_trace_probe
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
#include "ast.h"

/* Protected entry points, called with real object pointers. */
extern int astRegTrace_( void *, int, double *, double **, int * );
extern void **astRegSplit_( void *, int *, int * );
extern void *astFree_( void *, int * );

static const double dist_up[] = { 0.0, 0.125, 0.25, 0.5, 0.5, 0.75, 0.9999, 1.0 };
static const double dist_mixed[] = { 0.9, 0.1, 0.6, 0.3, 1.0, 0.0, 0.45 };
#define NUP ( (int)( sizeof( dist_up ) / sizeof( dist_up[ 0 ] ) ) )
#define NMIXED ( (int)( sizeof( dist_mixed ) / sizeof( dist_mixed[ 0 ] ) ) )
/* Non-decreasing but for a NaN, which no comparison finds out of order. */
static const double dist_nan[] = { 0.0, 0.1, 0.3, NAN, 0.5, 0.7, 0.9 };
#define NNAN ( (int)( sizeof( dist_nan ) / sizeof( dist_nan[ 0 ] ) ) )

static char dump[ 200000 ];
static int status = 0;

static void sink( const char *line ) {
   strcat( dump, line );
   strcat( dump, "\n" );
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

static void flit( double value ) {
   char buf[ 40 ];
   if( value == AST__BAD ) { fputs( "BAD", stdout ); return; }
   if( isnan( value ) ) { fputs( "f64::NAN", stdout ); return; }
   snprintf( buf, sizeof( buf ), "%.17g", value );
   fputs( buf, stdout );
   if( !strpbrk( buf, ".en" ) ) fputs( ".0", stdout );
}

static AstFrame *frame( int naxes ) {
   return astFrame( naxes, " " );
}

static AstFrame *sky( void ) {
   return (AstFrame *) astSkyFrame( "System=ICRS" );
}

static AstRegion *moc( const char *text ) {
   AstMoc *m = astMoc( " " );
   int json;
   if( text ) astAddMocString( m, AST__OR, 0, -1, strlen( text ), text, &json );
   return (AstRegion *) m;
}

static AstRegion *make_region( const char *id );

static AstRegion *galactic( const char *id ) {
   AstRegion *reg = make_region( id );
   astSet( reg, "System=Galactic" );
   return reg;
}

static AstRegion *negated( const char *id ) {
   AstRegion *reg = make_region( id );
   astNegate( reg );
   return reg;
}

static AstRegion *cmp( const char *a, const char *b, int oper ) {
   AstRegion *r1 = make_region( a );
   AstRegion *r2 = make_region( b );
   AstRegion *result = (AstRegion *) astCmpRegion( r1, r2, oper, " " );
   r1 = astAnnul( r1 );
   r2 = astAnnul( r2 );
   return result;
}

/* Each Region the Rust test builds again from the same id. */
static AstRegion *make_region( const char *id ) {
   if( !strcmp( id, "box2d" ) ) {
      double p1[] = { 0.0, 0.0 }, p2[] = { 2.0, 1.0 };
      return (AstRegion *) astBox( frame( 2 ), 1, p1, p2, NULL, " " );
   }
   if( !strcmp( id, "box_sky" ) ) {
      double p1[] = { 0.1, 0.2 }, p2[] = { 0.4, 0.5 };
      return (AstRegion *) astBox( sky(), 1, p1, p2, NULL, " " );
   }
   if( !strcmp( id, "box_sky_gal" ) ) return galactic( "box_sky" );
   if( !strcmp( id, "box3d" ) ) {
      double p1[] = { 0.0, 0.0, 0.0 }, p2[] = { 1.0, 1.0, 1.0 };
      return (AstRegion *) astBox( frame( 3 ), 1, p1, p2, NULL, " " );
   }
   if( !strcmp( id, "box_neg" ) ) return negated( "box2d" );
   if( !strcmp( id, "circle2d" ) ) {
      double c[] = { 1.0, 2.0 }, r[] = { 0.5 };
      return (AstRegion *) astCircle( frame( 2 ), 1, c, r, NULL, " " );
   }
   if( !strcmp( id, "circle_sky" ) ) {
      double c[] = { 1.0, 0.5 }, r[] = { 0.1 };
      return (AstRegion *) astCircle( sky(), 1, c, r, NULL, " " );
   }
   if( !strcmp( id, "circle_sky_gal" ) ) return galactic( "circle_sky" );
   if( !strcmp( id, "circle1d" ) ) {
      double c[] = { 1.0 }, r[] = { 0.5 };
      return (AstRegion *) astCircle( frame( 1 ), 1, c, r, NULL, " " );
   }
   if( !strcmp( id, "circle_neg" ) ) return negated( "circle2d" );
   if( !strcmp( id, "ellipse2d" ) ) {
      double c[] = { 0.0, 0.0 }, ab[] = { 2.0, 1.0 }, ang[] = { 0.5 };
      return (AstRegion *) astEllipse( frame( 2 ), 1, c, ab, ang, NULL, " " );
   }
   if( !strcmp( id, "ellipse_sky" ) ) {
      double c[] = { 1.0, 0.3 }, ab[] = { 0.1, 0.05 }, ang[] = { 0.3 };
      return (AstRegion *) astEllipse( sky(), 1, c, ab, ang, NULL, " " );
   }
   if( !strcmp( id, "ellipse_sky_gal" ) ) return galactic( "ellipse_sky" );
   if( !strcmp( id, "interval2d" ) ) {
      double lo[] = { 0.0, 0.0 }, hi[] = { 1.0, 2.0 };
      return (AstRegion *) astInterval( frame( 2 ), lo, hi, NULL, " " );
   }
   if( !strcmp( id, "interval_open" ) ) {
      double lo[] = { 0.0, AST__BAD }, hi[] = { 1.0, 2.0 };
      return (AstRegion *) astInterval( frame( 2 ), lo, hi, NULL, " " );
   }
   if( !strcmp( id, "interval_excl" ) ) {
      double lo[] = { 1.0, 2.0 }, hi[] = { 0.0, 0.0 };
      return (AstRegion *) astInterval( frame( 2 ), lo, hi, NULL, " " );
   }
   if( !strcmp( id, "interval1d" ) ) {
      double lo[] = { 0.0 }, hi[] = { 1.0 };
      return (AstRegion *) astInterval( frame( 1 ), lo, hi, NULL, " " );
   }
   if( !strcmp( id, "interval_sky_gal" ) ) {
      double lo[] = { 0.1, 0.2 }, hi[] = { 0.4, 0.5 };
      AstRegion *reg = (AstRegion *) astInterval( sky(), lo, hi, NULL, " " );
      astSet( reg, "System=Galactic" );
      return reg;
   }
   if( !strcmp( id, "polygon_quad" ) ) {
      double p[] = { 0.0, 2.0, 2.5, 0.0,  0.0, 0.0, 1.0, 1.5 };
      return (AstRegion *) astPolygon( frame( 2 ), 4, 4, p, NULL, " " );
   }
   if( !strcmp( id, "polygon_hept" ) ) {
      double p[] = { 0.0, 1.0, 2.0, 2.5, 2.0, 1.0, 0.2,
                     0.0, -0.5, 0.0, 1.0, 2.0, 1.8, 1.0 };
      return (AstRegion *) astPolygon( frame( 2 ), 7, 7, p, NULL, " " );
   }
   if( !strcmp( id, "polygon_sky" ) ) {
      double p[] = { 1.0, 1.1, 1.1, 1.0,  0.5, 0.5, 0.6, 0.62 };
      return (AstRegion *) astPolygon( sky(), 4, 4, p, NULL, " " );
   }
   if( !strcmp( id, "polygon_sky_gal" ) ) return galactic( "polygon_sky" );
   if( !strcmp( id, "polygon_neg" ) ) return negated( "polygon_quad" );
   if( !strcmp( id, "moc_cell" ) ) return moc( "3/25" );
   if( !strcmp( id, "moc_block" ) ) return moc( "4/96-111" );
   if( !strcmp( id, "moc_two" ) ) return moc( "3/25 3/600" );
   if( !strcmp( id, "moc_mixed" ) ) return moc( "2/6 3/28-29" );
   if( !strcmp( id, "moc_diag" ) ) return moc( "4/0 4/3" );
   if( !strcmp( id, "moc_empty" ) ) return moc( NULL );
   if( !strcmp( id, "moc_hole" ) ) return moc( "3/0-2,4-5,7-8,10-11,13-15" );
   if( !strcmp( id, "moc_hole_corner" ) ) return moc( "3/1-2,4-15" );
   if( !strcmp( id, "moc_touch" ) ) return moc( "3/0-3,12-15" );
   if( !strcmp( id, "moc_big" ) ) return moc( "4/0-2,4-15,17,19-63" );
   if( !strcmp( id, "moc_gal" ) ) return galactic( "moc_block" );
   if( !strcmp( id, "pointlist" ) ) {
      double p[] = { 0.0, 1.0,  0.0, 1.0 };
      return (AstRegion *) astPointList( frame( 2 ), 2, 2, 2, p, NULL, " " );
   }

   /* Components of the CmpRegions. */
   if( !strcmp( id, "cA" ) ) {
      double c[] = { 1.0, 1.0 }, r[] = { 1.0 };
      return (AstRegion *) astCircle( frame( 2 ), 1, c, r, NULL, " " );
   }
   if( !strcmp( id, "bA" ) ) {
      double p1[] = { 0.5, 0.5 }, p2[] = { 2.5, 1.5 };
      return (AstRegion *) astBox( frame( 2 ), 1, p1, p2, NULL, " " );
   }
   if( !strcmp( id, "cB" ) ) {
      double c[] = { 5.0, 5.0 }, r[] = { 1.0 };
      return (AstRegion *) astCircle( frame( 2 ), 1, c, r, NULL, " " );
   }
   if( !strcmp( id, "cC" ) ) {
      double c[] = { 1.8, 1.2 }, r[] = { 0.6 };
      return (AstRegion *) astCircle( frame( 2 ), 1, c, r, NULL, " " );
   }
   if( !strcmp( id, "neg_bA" ) ) return negated( "bA" );
   if( !strcmp( id, "sky_c1" ) ) {
      double c[] = { 1.0, 0.5 }, r[] = { 0.1 };
      return (AstRegion *) astCircle( sky(), 1, c, r, NULL, " " );
   }
   if( !strcmp( id, "sky_c2" ) ) {
      double c[] = { 1.5, 0.5 }, r[] = { 0.1 };
      return (AstRegion *) astCircle( sky(), 1, c, r, NULL, " " );
   }
   if( !strcmp( id, "sky_b1" ) ) {
      double p1[] = { 1.05, 0.45 }, p2[] = { 1.2, 0.55 };
      return (AstRegion *) astBox( sky(), 1, p1, p2, NULL, " " );
   }

   /* CmpRegions. */
   if( !strcmp( id, "and_cb" ) ) return cmp( "cA", "bA", AST__AND );
   if( !strcmp( id, "or_cb" ) ) return cmp( "cA", "bA", AST__OR );
   if( !strcmp( id, "xor_cb" ) ) return cmp( "cA", "bA", AST__XOR );
   if( !strcmp( id, "or_disjoint" ) ) return cmp( "cA", "cB", AST__OR );
   if( !strcmp( id, "and_negbox" ) ) return cmp( "cA", "neg_bA", AST__AND );
   if( !strcmp( id, "or_negbox" ) ) return cmp( "cA", "neg_bA", AST__OR );
   if( !strcmp( id, "or_disjoint_neg" ) ) return negated( "or_disjoint" );
   if( !strcmp( id, "nested" ) ) return cmp( "or_cb", "cC", AST__AND );
   if( !strcmp( id, "or_three" ) ) return cmp( "or_disjoint", "cC", AST__OR );
   if( !strcmp( id, "or_sky" ) ) return cmp( "sky_c1", "sky_b1", AST__OR );
   if( !strcmp( id, "or_sky_gal" ) ) return galactic( "or_sky" );
   if( !strcmp( id, "or_sky_disjoint" ) ) return cmp( "sky_c1", "sky_c2", AST__OR );
   if( !strcmp( id, "or_sky_disjoint_gal" ) ) return galactic( "or_sky_disjoint" );
   if( !strcmp( id, "and_sky_gal" ) ) {
      AstRegion *reg = cmp( "sky_c1", "sky_b1", AST__AND );
      astSet( reg, "System=Galactic" );
      return reg;
   }
   if( !strcmp( id, "or_unbounded" ) ) return cmp( "cA", "interval_open", AST__OR );
   if( !strcmp( id, "or_pointlist2" ) ) return cmp( "cA", "pointlist", AST__OR );
   if( !strcmp( id, "or_pointlist1" ) ) return cmp( "pointlist", "cA", AST__OR );

   printf( "unknown region id %s\n", id );
   exit( 1 );
}

static const char *trace_ids[] = {
   "box2d", "box_sky", "box_sky_gal", "box3d", "box_neg",
   "circle2d", "circle_sky", "circle_sky_gal", "circle1d", "circle_neg",
   "ellipse2d", "ellipse_sky", "ellipse_sky_gal",
   "interval2d", "interval_open", "interval_excl", "interval1d", "interval_sky_gal",
   "polygon_quad", "polygon_hept", "polygon_sky", "polygon_sky_gal", "polygon_neg",
   "moc_cell", "moc_block", "moc_two", "moc_mixed", "moc_diag", "moc_empty", "moc_gal",
   "moc_hole", "moc_hole_corner", "moc_touch", "moc_big",
   "pointlist",
};

static const char *cmp_trace_ids[] = {
   "and_cb", "or_cb", "xor_cb", "or_disjoint", "and_negbox", "or_negbox", "nested",
   "or_three", "or_sky", "or_sky_gal", "or_pointlist1",
};

/* CmpRegions C claims to trace but whose positions it reads from arrays the
   second component never fills; only the support flag is defined. */
static const char *cmp_undefined_ids[] = { "or_pointlist2" };

/* Regions traced at dist_nan: a Polygon with enough vertices to test
   whether the distances are in order. */
static const char *nan_ids[] = { "polygon_hept" };

static const char *split_ids[] = {
   "circle2d", "or_cb", "or_disjoint", "or_disjoint_neg", "or_three", "and_cb", "or_negbox",
   "or_unbounded", "or_sky_disjoint", "or_sky_disjoint_gal", "and_sky_gal",
};

#define COUNT(a) ( (int)( sizeof( a ) / sizeof( a[ 0 ] ) ) )

static void positions( AstRegion *reg, int n, const double *dist ) {
   int ncur = astGetI( reg, "Naxes" );
   double *ptr[ 3 ];
   int i, j;
   for( j = 0; j < ncur; j++ ) ptr[ j ] = malloc( n*sizeof( double ) );
   astRegTrace_( astMakePointer( reg ), n, (double *) dist, ptr, &status );
   printf( "&[" );
   for( j = 0; j < ncur; j++ ) {
      printf( "&[" );
      for( i = 0; i < n; i++ ) {
         flit( ptr[ j ][ i ] );
         if( i < n - 1 ) printf( ", " );
      }
      printf( "]%s", j < ncur - 1 ? ", " : "" );
      free( ptr[ j ] );
   }
   printf( "]" );
}

static void trace_cases( const char *name, const char **ids, int nid ) {
   int k;
   printf( "pub const %s: &[TraceCase] = &[\n", name );
   for( k = 0; k < nid; k++ ) {
      AstRegion *reg = make_region( ids[ k ] );
      int ok = astRegTrace_( astMakePointer( reg ), 0, NULL, NULL, &status );
      printf( "    (\"%s\", %s, ", ids[ k ], ok ? "true" : "false" );
      if( ok ) {
         positions( reg, NUP, dist_up );
         printf( ", " );
         positions( reg, NMIXED, dist_mixed );
      } else {
         printf( "&[], &[]" );
      }
      printf( "),\n" );
      reg = astAnnul( reg );
      if( !astOK ) {
         fprintf( stderr, "error in %s\n", ids[ k ] );
         exit( 1 );
      }
   }
   printf( "];\n\n" );
}

int main( void ) {
   int k, i, nlist;
   astWatch( &status );

   printf( "// Generated by tools/oracle_fixtures/region_trace_probe.c.\n"
           "// Do not edit.\n\n"
           "// Each value is printed with 17 digits so that it reads back exactly.\n"
           "#![allow(clippy::excessive_precision, clippy::approx_constant)]\n\n"
           "use starlink_ast::constants::BAD_VALUE as BAD;\n\n"
           "/// `(region id, supported, positions at DIST_UP, positions at DIST_MIXED)`,\n"
           "/// the positions as one array per current-Frame axis.\n"
           "pub type TraceCase = (&'static str, bool, &'static [&'static [f64]], "
           "&'static [&'static [f64]]);\n\n" );

   printf( "/// Non-decreasing distances, with a repeat.\n"
           "pub const DIST_UP: &[f64] = &[" );
   for( i = 0; i < NUP; i++ ) { flit( dist_up[ i ] ); printf( i < NUP - 1 ? ", " : "" ); }
   printf( "];\n\n/// Distances out of order.\npub const DIST_MIXED: &[f64] = &[" );
   for( i = 0; i < NMIXED; i++ ) {
      flit( dist_mixed[ i ] ); printf( i < NMIXED - 1 ? ", " : "" );
   }
   printf( "];\n\n/// Non-decreasing distances but for a NaN.\npub const DIST_NAN: &[f64] = &[" );
   for( i = 0; i < NNAN; i++ ) { flit( dist_nan[ i ] ); printf( i < NNAN - 1 ? ", " : "" ); }
   printf( "];\n\n" );

   printf( "/// Regions of the classes with their own tracing, and some without.\n" );
   trace_cases( "TRACE_CASES", trace_ids, COUNT( trace_ids ) );
   printf( "/// CmpRegions.\n" );
   trace_cases( "CMP_TRACE_CASES", cmp_trace_ids, COUNT( cmp_trace_ids ) );

   printf( "/// `(region id, supported)` for CmpRegions whose positions C reads\n"
           "/// from arrays it never fills.\n"
           "pub const CMP_UNDEFINED_CASES: &[(&str, bool)] = &[\n" );
   for( k = 0; k < COUNT( cmp_undefined_ids ); k++ ) {
      AstRegion *reg = make_region( cmp_undefined_ids[ k ] );
      int ok = astRegTrace_( astMakePointer( reg ), 0, NULL, NULL, &status );
      printf( "    (\"%s\", %s),\n", cmp_undefined_ids[ k ], ok ? "true" : "false" );
      reg = astAnnul( reg );
   }
   printf( "];\n\n" );

   printf( "/// `(region id, positions at DIST_NAN)`.\n"
           "pub const NAN_CASES: &[(&str, &[&[f64]])] = &[\n" );
   for( k = 0; k < COUNT( nan_ids ); k++ ) {
      AstRegion *reg = make_region( nan_ids[ k ] );
      printf( "    (\"%s\", ", nan_ids[ k ] );
      positions( reg, NNAN, dist_nan );
      printf( "),\n" );
      reg = astAnnul( reg );
   }
   printf( "];\n\n" );

   printf( "/// `(region id, Native dump of each part)` from astRegSplit.\n"
           "pub const SPLIT_CASES: &[(&str, &[&str])] = &[\n" );
   for( k = 0; k < COUNT( split_ids ); k++ ) {
      AstRegion *reg = make_region( split_ids[ k ] );
      void **parts = astRegSplit_( astMakePointer( reg ), &nlist, &status );
      printf( "    (\"%s\", &[\n", split_ids[ k ] );
      for( i = 0; i < nlist; i++ ) {
         AstObject *part = astMakeId_( parts[ i ], &status );
         AstChannel *ch = astChannel( NULL, sink, " " );
         dump[ 0 ] = 0;
         astWrite( ch, part );
         ch = astAnnul( ch );
         part = astAnnul( part );
         printf( "        " );
         str( dump );
         printf( ",\n" );
      }
      printf( "    ]),\n" );
      astFree_( parts, &status );
      reg = astAnnul( reg );
      if( !astOK ) {
         fprintf( stderr, "error in %s\n", split_ids[ k ] );
         exit( 1 );
      }
   }
   printf( "];\n" );
   return 0;
}
