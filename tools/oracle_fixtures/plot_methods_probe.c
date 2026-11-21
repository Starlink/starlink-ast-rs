/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* plot_methods_probe -- drive the Plot drawing methods that C's own tests do
 * not check (plot.c Curve, PolyCurve, GenCurve, Mark, Clip, RegionOutline
 * and BoundingBox) through C's grf_log.c, and capture the call log and SVG.
 *
 * Each case is a Plot of a header or of a 2-D Frame, with attributes set,
 * and a list of operations run in order under one capture; after them the
 * probe records astBoundingBox. For some cases it also records the Plot's
 * Native dump. Mappings for astGenCurve and Regions for astRegionOutline
 * are named by an id, from which the Rust test builds the same object.
 *
 * Prints Rust source for crates/starlink-ast/tests/plot_methods_oracle/cases.rs.
 * Build and run from reference/starlink-ast/build as plot_curve_probe.c's
 * header describes, with this file in place of plot_curve_probe.c:
 *
 *   ../../../tools/oracle_fixtures/plot_methods_probe \
 *      > ../../../crates/starlink-ast/tests/plot_methods_oracle/cases.rs
 */
#include "plot_draw_probe.h"

enum { OP_CURVE, OP_POLY, OP_GEN, OP_MARK, OP_CLIP, OP_OUTLINE };

#define BAD AST__BAD
#define NOFRAME AST__NOFRAME

/* One operation. For a curve, "a" is its start and "b" its finish; for a
   polycurve or marks, "a" and "b" hold the n values on axes 1 and 2; for a
   clip, "a" and "b" are the lower and upper bounds in Frame "frame". */
struct op {
   int kind;
   int n;
   double a[ 8 ];
   double b[ 8 ];
   int marker;
   int frame;
   const char *id;
};

struct method_case {
   const char *name;
   const char *head;      /* header under HEADS, or NULL for astFrame(2) */
   float gbox[ 4 ];
   double pbox[ 4 ];
   const char *attr;
   int dump;              /* record the Native dump after the operations? */
   int nop;
   struct op ops[ 6 ];
};

#define FRAME_BOX { 0.1f, 0.1f, 0.9f, 0.9f }, { 0.0, 0.0, 10.0, 10.0 }
#define AITOFF_BOX { 0.1f, 0.1f, 0.9f, 0.9f }, { 10.0, -10.0, 300.0, 280.0 }

#define CURVE( x0, y0, x1, y1 ) { OP_CURVE, 2, { x0, y0 }, { x1, y1 }, 0, 0, NULL }
#define GEN( id ) { OP_GEN, 0, { 0 }, { 0 }, 0, 0, id }
#define OUTLINE( id ) { OP_OUTLINE, 0, { 0 }, { 0 }, 0, 0, id }
#define CLIP( f, l0, l1, u0, u1 ) { OP_CLIP, 0, { l0, l1 }, { u0, u1 }, 0, f, NULL }
/* A curve, a polycurve and markers that cross the clip boxes below. */
#define FRAME_DRAWING CURVE( 1.0, 1.0, 9.0, 9.0 ), \
   { OP_POLY, 3, { 1.0, 5.0, 9.0 }, { 9.0, 1.0, 9.0 }, 0, 0, NULL }, \
   { OP_MARK, 5, { 2.0, 5.0, 8.0, 5.0, 2.0 }, { 2.0, 5.0, 8.0, 2.0, 8.0 }, 1, 0, NULL }
/* A curve and markers that run past the plotting area but stay partly
   inside a clip box from -1 to 11. */
#define WIDE_DRAWING CURVE( -3.0, -3.0, 13.0, 13.0 ), \
   { OP_MARK, 5, { -2.0, -0.5, 5.0, 10.5, 12.0 }, { 5.0, 5.0, 5.0, 5.0, 5.0 }, 1, 0, NULL }

static const struct method_case cases[] = {
   { "frame_curve", NULL, FRAME_BOX, "", 0, 3,
     { CURVE( 1.0, 1.0, 9.0, 5.0 ), CURVE( 2.0, 8.0, 6.0, 3.0 ),
       CURVE( -4.0, 5.0, 14.0, 7.0 ) } },
   { "frame_curve_styles", NULL, FRAME_BOX,
     "Colour(curves)=2,Width(curves)=3,Style(curves)=2", 0, 1,
     { CURVE( 1.0, 1.0, 9.0, 5.0 ) } },
   { "frame_curve_log", NULL, { 0.1f, 0.1f, 0.9f, 0.9f },
     { 1.0, 0.0, 1000.0, 10.0 }, "LogPlot(1)=1", 0, 1,
     { CURVE( 2.0, 1.0, 900.0, 9.0 ) } },
   { "frame_curve_bad", NULL, FRAME_BOX, "", 0, 1,
     { CURVE( BAD, 1.0, 9.0, 5.0 ) } },
   { "frame_curve_invisible", NULL, FRAME_BOX, "Invisible=1", 0, 1,
     { CURVE( 1.0, 1.0, 9.0, 5.0 ) } },
   { "aitoff_curve", "aitoff.head", AITOFF_BOX, "", 0, 2,
     { CURVE( 0.5, 0.2, 1.5, 0.6 ), CURVE( -2.5, 0.0, 2.5, 0.3 ) } },
   { "aitoff_curve_tol", "aitoff.head", AITOFF_BOX, "Tol=0.001", 0, 1,
     { CURVE( -2.5, 0.0, 2.5, 0.3 ) } },
   { "frame_poly", NULL, FRAME_BOX, "", 0, 1,
     { { OP_POLY, 5, { 1.0, 3.0, BAD, 7.0, 9.0 }, { 1.0, 8.0, 5.0, 2.0, 6.0 },
         0, 0, NULL } } },
   { "frame_poly_joined", NULL, FRAME_BOX, "", 0, 1,
     { { OP_POLY, 4, { 1.0, 5.0, 9.0, 5.0 }, { 1.0, 8.0, 2.0, 1.0 }, 0, 0, NULL } } },
   { "frame_poly_one", NULL, FRAME_BOX, "", 0, 1,
     { { OP_POLY, 1, { 1.0 }, { 1.0 }, 0, 0, NULL } } },
   { "aitoff_poly", "aitoff.head", AITOFF_BOX, "", 0, 1,
     { { OP_POLY, 4, { 0.5, 1.5, 2.8, -2.8 }, { 0.2, 0.6, 0.1, -0.2 },
         0, 0, NULL } } },
   { "frame_gen_curve", NULL, FRAME_BOX, "", 0, 1, { GEN( "parabola" ) } },
   { "frame_gen_curve_styles", NULL, FRAME_BOX,
     "Colour(curves)=2,Width(curves)=3,Style(curves)=2", 0, 1, { GEN( "parabola" ) } },
   { "aitoff_gen_curve", "aitoff.head", AITOFF_BOX, "", 0, 1, { GEN( "sky_arc" ) } },
   { "frame_mark", NULL, FRAME_BOX, "", 0, 2,
     { { OP_MARK, 5, { 1.0, 5.0, BAD, 12.0, 8.0 }, { 1.0, 5.0, 3.0, 5.0, 9.0 },
         1, 0, NULL },
       { OP_MARK, 3, { 2.0, 4.0, 6.0 }, { 7.0, 7.5, 8.0 }, 3, 0, NULL } } },
   { "frame_mark_clip", NULL, FRAME_BOX, "Clip=2", 0, 1,
     { { OP_MARK, 5, { 1.0, 5.0, BAD, 12.0, 8.0 }, { 1.0, 5.0, 3.0, 5.0, 9.0 },
         1, 0, NULL } } },
   { "frame_mark_styles", NULL, FRAME_BOX, "Colour(markers)=4,Size(markers)=2", 0, 1,
     { { OP_MARK, 3, { 2.0, 4.0, 6.0 }, { 7.0, 7.5, 8.0 }, 3, 0, NULL } } },
   { "frame_mark_invisible", NULL, FRAME_BOX, "Invisible=1", 0, 1,
     { { OP_MARK, 3, { 2.0, 4.0, 6.0 }, { 7.0, 7.5, 8.0 }, 3, 0, NULL } } },
   { "aitoff_mark", "aitoff.head", AITOFF_BOX, "", 0, 1,
     { { OP_MARK, 4, { 0.5, 1.5, -2.0, 3.5 }, { 0.2, 0.6, -0.4, 0.0 }, 2, 0, NULL } } },
   { "frame_clip_base", NULL, FRAME_BOX, "", 0, 4,
     { CLIP( AST__BASE, 0.3, 0.3, 0.7, 0.7 ), FRAME_DRAWING } },
   { "frame_clip_current", NULL, FRAME_BOX, "", 1, 4,
     { CLIP( AST__CURRENT, 3.0, BAD, 7.0, 6.0 ), FRAME_DRAWING } },
   { "frame_clip_op", NULL, FRAME_BOX, "ClipOp=1", 0, 4,
     { CLIP( AST__CURRENT, 3.0, 3.0, 7.0, 7.0 ), FRAME_DRAWING } },
   { "frame_clip_curves", NULL, FRAME_BOX, "Clip=1", 0, 3,
     { CLIP( AST__CURRENT, -1.0, -1.0, 11.0, 11.0 ), WIDE_DRAWING } },
   { "frame_clip_marks", NULL, FRAME_BOX, "Clip=2", 0, 3,
     { CLIP( AST__CURRENT, -1.0, -1.0, 11.0, 11.0 ), WIDE_DRAWING } },
   { "frame_clip_none", NULL, FRAME_BOX, "Clip=0", 0, 3,
     { CLIP( AST__CURRENT, -1.0, -1.0, 11.0, 11.0 ), WIDE_DRAWING } },
   { "frame_clip_cleared", NULL, FRAME_BOX, "", 0, 5,
     { CLIP( AST__CURRENT, 3.0, 3.0, 7.0, 7.0 ), CLIP( NOFRAME, 0.0, 0.0, 0.0, 0.0 ),
       FRAME_DRAWING } },
   { "frame_outline_simple", NULL, FRAME_BOX, "", 1, 4,
     { OUTLINE( "box" ), OUTLINE( "circle" ), OUTLINE( "ellipse" ),
       OUTLINE( "interval" ) } },
   { "frame_outline_polygon", NULL, FRAME_BOX, "", 0, 1, { OUTLINE( "polygon" ) } },
   { "frame_outline_cmp", NULL, FRAME_BOX, "", 0, 2,
     { OUTLINE( "and_overlap" ), OUTLINE( "or_disjoint" ) } },
   { "frame_outline_open", NULL, FRAME_BOX, "", 0, 1, { OUTLINE( "interval_open" ) } },
   /* The clip box cuts the circle into four arcs, each shorter than the
      spacing of Crv's first samples, so no segment has two good ends and
      Crv draws nothing (c-library-quirks.md). The short-arcs case shows the
      same with a box in the current Frame. */
   { "frame_outline_clipped", NULL, FRAME_BOX, "", 0, 2,
     { CLIP( AST__BASE, 0.3, 0.3, 0.7, 0.7 ), OUTLINE( "circle" ) } },
   { "frame_outline_short_arcs", NULL, FRAME_BOX, "", 0, 2,
     { CLIP( AST__CURRENT, 2.5, 2.5, 7.5, 7.5 ), OUTLINE( "circle" ) } },
   { "frame_outline_clipped_current", NULL, FRAME_BOX, "", 0, 2,
     { CLIP( AST__CURRENT, 0.0, 0.0, 5.0, 10.0 ), OUTLINE( "circle" ) } },
   { "aitoff_outline", "aitoff.head", AITOFF_BOX, "", 0, 4,
     { OUTLINE( "sky_circle" ), OUTLINE( "sky_polygon" ), OUTLINE( "icrs_circle" ),
       OUTLINE( "moc" ) } },
   { "aitoff_clip", "aitoff.head", AITOFF_BOX, "", 1, 3,
     { CLIP( 3, 2.0, -0.2, 2.8, 0.4 ), CURVE( 1.8, 0.0, 3.0, 0.3 ),
       { OP_MARK, 4, { 1.9, 2.2, 2.6, 2.9 }, { 0.0, 0.1, 0.2, 0.3 }, 1, 0, NULL } } },
   /* Frame 2 is the pixel Frame, neither the base nor the current Frame. */
   { "aitoff_clip_pixel", "aitoff.head", AITOFF_BOX, "", 0, 3,
     { CLIP( 2, 80.0, BAD, 230.0, BAD ), CURVE( -2.5, 0.0, 2.5, 0.3 ),
       { OP_MARK, 4, { 0.5, 1.5, -2.0, 3.5 }, { 0.2, 0.6, -0.4, 0.0 }, 2, 0, NULL } } },
};

/* The ClipFrame a frame index is written as. */
static void clip_frame( int frame ) {
   if( frame == NOFRAME ) printf( "ClipFrame::None" );
   else if( frame == AST__BASE ) printf( "ClipFrame::Base" );
   else if( frame == AST__CURRENT ) printf( "ClipFrame::Current" );
   else printf( "ClipFrame::Index(%d)", frame );
}

static void values( const double *v, int n ) {
   int i;
   printf( "&[" );
   for( i = 0; i < n; i++ ) {
      if( i ) printf( ", " );
      flit( v[ i ] );
   }
   printf( "]" );
}

static void print_op( const struct op *op ) {
   switch( op->kind ) {
   case OP_CURVE:
      printf( "Op::Curve(" ); values( op->a, 2 ); printf( ", " );
      values( op->b, 2 ); printf( ")" );
      break;
   case OP_POLY:
      printf( "Op::PolyCurve(&[" ); values( op->a, op->n ); printf( ", " );
      values( op->b, op->n ); printf( "])" );
      break;
   case OP_GEN:
      printf( "Op::GenCurve(" ); rstr( op->id ); printf( ")" );
      break;
   case OP_MARK:
      printf( "Op::Mark(&[" ); values( op->a, op->n ); printf( ", " );
      values( op->b, op->n ); printf( "], %d)", op->marker );
      break;
   case OP_CLIP:
      printf( "Op::Clip(" ); clip_frame( op->frame ); printf( ", " );
      values( op->a, 2 ); printf( ", " ); values( op->b, 2 ); printf( ")" );
      break;
   case OP_OUTLINE:
      printf( "Op::Outline(" ); rstr( op->id ); printf( ")" );
      break;
   }
}

/* The Mapping astGenCurve draws for an id. */
static AstMapping *make_map( const char *id ) {
   /* Each term is a coefficient, its 1-based output and the power of t. */
   if( !strcmp( id, "parabola" ) ) {
      double c[] = { 1.0, 1, 0,  8.0, 1, 1,  2.0, 2, 0,  6.0, 2, 2 };
      return (AstMapping *) astPolyMap( 1, 2, 4, c, 0, NULL, "" );
   }
   if( !strcmp( id, "sky_arc" ) ) {
      double c[] = { 0.5, 1, 0,  1.0, 1, 1,  0.2, 2, 0,  0.4, 2, 1 };
      return (AstMapping *) astPolyMap( 1, 2, 4, c, 0, NULL, "" );
   }
   fprintf( stderr, "unknown Mapping %s\n", id );
   exit( 1 );
}

/* The Region astRegionOutline draws for an id. */
static AstRegion *circle( AstFrame *f, double x, double y, double r ) {
   double c[ 2 ];
   c[ 0 ] = x;
   c[ 1 ] = y;
   return (AstRegion *) astCircle( f, 1, c, &r, NULL, "" );
}

static AstRegion *make_region( const char *id ) {
   AstFrame *f2 = astFrame( 2, "" );
   AstFrame *gal = (AstFrame *) astSkyFrame( "System=Galactic" );
   AstFrame *icrs = (AstFrame *) astSkyFrame( "System=ICRS" );
   AstRegion *reg = NULL;
   if( !strcmp( id, "box" ) ) {
      double p1[] = { 2.0, 3.0 }, p2[] = { 7.0, 6.0 };
      reg = (AstRegion *) astBox( f2, 1, p1, p2, NULL, "" );
   } else if( !strcmp( id, "circle" ) ) {
      reg = circle( f2, 5.0, 5.0, 3.0 );
   } else if( !strcmp( id, "ellipse" ) ) {
      double c[] = { 5.0, 5.0 }, ab[] = { 3.0, 1.5 }, ang = 0.5;
      reg = (AstRegion *) astEllipse( f2, 1, c, ab, &ang, NULL, "" );
   } else if( !strcmp( id, "interval" ) ) {
      double lo[] = { 2.0, 3.0 }, hi[] = { 8.0, 7.0 };
      reg = (AstRegion *) astInterval( f2, lo, hi, NULL, "" );
   } else if( !strcmp( id, "interval_open" ) ) {
      double lo[] = { 2.0, BAD }, hi[] = { 8.0, 7.0 };
      reg = (AstRegion *) astInterval( f2, lo, hi, NULL, "" );
   } else if( !strcmp( id, "polygon" ) ) {
      double p[] = { 2.0, 5.0, 8.0, 9.5, 8.0, 5.0, 2.6,
                     2.0, 0.5, 2.0, 5.0, 8.0, 7.4, 5.0 };
      reg = (AstRegion *) astPolygon( f2, 7, 7, p, NULL, "" );
   } else if( !strcmp( id, "and_overlap" ) || !strcmp( id, "or_disjoint" ) ) {
      int and = !strcmp( id, "and_overlap" );
      AstRegion *a = and ? circle( f2, 4.0, 5.0, 2.0 ) : circle( f2, 3.0, 3.0, 1.5 );
      AstRegion *b = and ? circle( f2, 6.0, 5.0, 2.0 ) : circle( f2, 7.0, 7.0, 1.5 );
      reg = (AstRegion *) astCmpRegion( a, b, and ? AST__AND : AST__OR, "" );
      a = astAnnul( a );
      b = astAnnul( b );
   } else if( !strcmp( id, "sky_circle" ) ) {
      reg = circle( gal, -2.75, -0.45, 0.1 );
   } else if( !strcmp( id, "sky_polygon" ) ) {
      double p[] = { -2.3, -2.2, -2.4, -2.5,  -0.2, -0.4, -0.5, -0.3 };
      reg = (AstRegion *) astPolygon( gal, 4, 4, p, NULL, "" );
   } else if( !strcmp( id, "icrs_circle" ) ) {
      reg = circle( icrs, 1.512434, -0.106751, 0.08 );
   } else if( !strcmp( id, "moc" ) ) {
      const char *text = "3/356,358 4/1413,1415,1421,1436";
      int json;
      reg = (AstRegion *) astMoc( "" );
      astAddMocString( (AstMoc *) reg, AST__OR, 0, -1, strlen( text ), text, &json );
   } else {
      fprintf( stderr, "unknown Region %s\n", id );
      exit( 1 );
   }
   f2 = astAnnul( f2 );
   gal = astAnnul( gal );
   icrs = astAnnul( icrs );
   return reg;
}

static void run_op( AstPlot *pl, const struct op *op ) {
   double in[ 16 ];
   int i;
   switch( op->kind ) {
   case OP_CURVE:
      astCurve( pl, op->a, op->b );
      break;
   case OP_POLY:
   case OP_MARK:
      for( i = 0; i < op->n; i++ ) {
         in[ i ] = op->a[ i ];
         in[ op->n + i ] = op->b[ i ];
      }
      if( op->kind == OP_POLY ) {
         astPolyCurve( pl, op->n, 2, op->n, in );
      } else {
         astMark( pl, op->n, 2, op->n, in, op->marker );
      }
      break;
   case OP_GEN: {
      AstMapping *map = make_map( op->id );
      astGenCurve( pl, map );
      map = astAnnul( map );
      break;
   }
   case OP_CLIP:
      astClip( pl, op->frame, op->a, op->b );
      break;
   case OP_OUTLINE: {
      AstRegion *reg = make_region( op->id );
      astRegionOutline( pl, reg );
      reg = astAnnul( reg );
      break;
   }
   }
}

/* The Native dump of an object. */
static char dump_buf[ 200000 ];
static size_t dump_len;
static void sink( const char *line ) {
   size_t n = strlen( line );
   if( dump_len + n + 2 < sizeof( dump_buf ) ) {
      memcpy( dump_buf + dump_len, line, n );
      dump_len += n;
      dump_buf[ dump_len++ ] = '\n';
      dump_buf[ dump_len ] = 0;
   }
}

static AstPlot *make_plot( const struct method_case *c ) {
   AstPlot *pl;
   if( c->head ) {
      char path[ 512 ];
      AstFrameSet *fs;
      snprintf( path, sizeof( path ), "%s%s", HEADS, c->head );
      fs = read_head( path, "" );
      pl = astPlot( fs, c->gbox, c->pbox, "" );
      fs = astAnnul( fs );
   } else {
      AstFrame *fr = astFrame( 2, "" );
      pl = astPlot( fr, c->gbox, c->pbox, "" );
      fr = astAnnul( fr );
   }
   astSet( pl, "%s", c->attr );
   return pl;
}

int main( void ) {
   int status_value = 0;
   int *status = &status_value;
   size_t icase;
   int i;
   float lbnd[ 2 ], ubnd[ 2 ];

   astWatch( status );
   printf( "// Generated by tools/oracle_fixtures/plot_methods_probe.c.\n"
           "// Do not edit.\n\n"
           "#![allow(clippy::excessive_precision, clippy::approx_constant)]\n\n"
           "/// The Frame of a clip operation.\n"
           "pub enum ClipFrame {\n"
           "    None,\n"
           "    Base,\n"
           "    Current,\n"
           "    Index(usize),\n"
           "}\n\n"
           "/// One drawing operation. Positions are given as one slice per\n"
           "/// axis.\n"
           "pub enum Op {\n"
           "    /// A geodesic curve from the first position to the second.\n"
           "    Curve(&'static [f64], &'static [f64]),\n"
           "    /// A polycurve through the positions.\n"
           "    PolyCurve(&'static [&'static [f64]]),\n"
           "    /// The curve given by the Mapping of an id.\n"
           "    GenCurve(&'static str),\n"
           "    /// Markers of a type at the positions.\n"
           "    Mark(&'static [&'static [f64]], i32),\n"
           "    /// A clip region in a Frame with lower and upper bounds.\n"
           "    Clip(ClipFrame, &'static [f64], &'static [f64]),\n"
           "    /// The outline of the Region of an id.\n"
           "    Outline(&'static str),\n"
           "}\n\n"
           "/// A Plot, the operations drawn on it, what C's grf_log wrote, and\n"
           "/// the bounding box afterwards (lower x, lower y, upper x, upper y).\n"
           "pub struct Case {\n"
           "    pub name: &'static str,\n"
           "    /// A header under fixtures/wcsconv/inputs, or None for a\n"
           "    /// 2-D Frame.\n"
           "    pub head: Option<&'static str>,\n"
           "    pub gbox: [f32; 4],\n"
           "    pub pbox: [f64; 4],\n"
           "    pub attr: &'static str,\n"
           "    pub ops: &'static [Op],\n"
           "    pub log: &'static str,\n"
           "    pub svg: Svg,\n"
           "    pub bbox: [f32; 4],\n"
           "}\n\n"
           SVG_DECL
           "pub const CASES: &[Case] = &[\n" );

   for( icase = 0; icase < sizeof( cases ) / sizeof( cases[ 0 ] ); icase++ ) {
      const struct method_case *c = cases + icase;
      AstPlot *pl = make_plot( c );

      printf( "    Case {\n        name: " );
      rstr( c->name );
      printf( ",\n        head: " );
      if( c->head ) { printf( "Some(" ); rstr( c->head ); printf( ")" ); }
      else printf( "None" );
      printf( ",\n        gbox: [" );
      for( i = 0; i < 4; i++ ) { if( i ) printf( ", " ); flit( c->gbox[ i ] ); }
      printf( "],\n        pbox: [" );
      for( i = 0; i < 4; i++ ) { if( i ) printf( ", " ); flit( c->pbox[ i ] ); }
      printf( "],\n        attr: " );
      rstr( c->attr );
      printf( ",\n        ops: &[\n" );
      for( i = 0; i < c->nop; i++ ) {
         printf( "            " );
         print_op( c->ops + i );
         printf( ",\n" );
      }
      printf( "        ],\n" );

      capture_start();
      for( i = 0; i < c->nop; i++ ) run_op( pl, c->ops + i );
      capture_end();
      astBoundingBox( pl, lbnd, ubnd );
      printf( "        bbox: [" );
      flit( lbnd[ 0 ] ); printf( ", " ); flit( lbnd[ 1 ] ); printf( ", " );
      flit( ubnd[ 0 ] ); printf( ", " ); flit( ubnd[ 1 ] );
      printf( "],\n    },\n" );
      if( !astOK ) {
         fprintf( stderr, "case %s failed with status %d\n", c->name, astStatus );
         return 1;
      }
      pl = astAnnul( pl );
   }
   printf( "];\n\n" );

   printf( "/// `(case name, Native dump of the Plot after its operations)`.\n"
           "pub const DUMPS: &[(&str, &str)] = &[\n" );
   for( icase = 0; icase < sizeof( cases ) / sizeof( cases[ 0 ] ); icase++ ) {
      const struct method_case *c = cases + icase;
      AstPlot *pl;
      AstChannel *ch;
      if( !c->dump ) continue;
      pl = make_plot( c );
      capture_start();
      for( i = 0; i < c->nop; i++ ) run_op( pl, c->ops + i );
      astGrfLogClose();
      ch = astChannel( NULL, sink, " " );
      dump_len = 0;
      dump_buf[ 0 ] = 0;
      astWrite( ch, pl );
      ch = astAnnul( ch );
      printf( "    (" );
      rstr( c->name );
      printf( ", r##\"%s\"##),\n", dump_buf );
      pl = astAnnul( pl );
   }
   printf( "];\n" );
   return 0;
}
