/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* plot_border_probe -- draw the border of each grid test Plot with astBorder
 * (plot.c Border, LinePlot, Map2, Boundary, TraceBorder, MakeGrid,
 * GraphGrid) through C's grf_log.c, and capture its result, call log and SVG.
 *
 * The Plots are those of ast_tester/fixtures/plot/cases.txt, built as
 * ast_tester/testgrid.c builds them, plus Plots of a 2-D Frame, and the car3
 * Plot with a 3-D current Frame added (a CmpFrame of its sky Frame and a
 * 1-D Frame), which Border and Grid reduce to its first two axes; for that
 * Plot the probe also records astGrid's call log and SVG.
 *
 * It also draws the border and grid of Plots whose current Frame is a
 * Region (plot.c DrawRegion and Map5, with the Region's RegTrace and
 * RegSplit, and Boundary's Region branch for a Region that cannot be
 * traced).
 *
 * Prints Rust source for crates/starlink-ast/tests/plot_border_oracle/cases.rs:
 *
 *   ./plot_border_probe > ../../crates/starlink-ast/tests/plot_border_oracle/cases.rs
 *
 * Build and run from reference/starlink-ast/build as plot_curve_probe.c's
 * header describes, with this file in place of plot_curve_probe.c.
 */
#include "plot_draw_probe.h"

/* The Plots whose current Frame is a Region, each of a 2-D Frame with a
   plot box of 0 to 10 on each axis unless the Region is on the sky (the
   Galactic Circle is defined in ICRS, so its Region holds a Mapping). The
   Rust test builds them again from the same name. */
static AstPlot *region_plot( const char *name ) {
   float gbox[ 4 ] = { 0.1f, 0.1f, 0.9f, 0.9f };
   double pbox[ 4 ] = { 0.0, 0.0, 10.0, 10.0 };
   AstFrame *f2 = astFrame( 2, "" );
   AstRegion *reg = NULL;
   AstPlot *pl;
   if( !strcmp( name, "region_circle" ) ) {
      double c[] = { 5.0, 5.0 }, r = 3.0;
      reg = (AstRegion *) astCircle( f2, 1, c, &r, NULL, "" );
   } else if( !strcmp( name, "region_sky_circle" ) ) {
      double c[] = { 1.0, 0.5 }, r = 0.05;
      AstFrame *sky = (AstFrame *) astSkyFrame( "System=ICRS" );
      reg = (AstRegion *) astCircle( sky, 1, c, &r, NULL, "" );
      pbox[ 0 ] = 0.9; pbox[ 1 ] = 0.4; pbox[ 2 ] = 1.1; pbox[ 3 ] = 0.6;
      sky = astAnnul( sky );
   } else if( !strcmp( name, "region_box" ) ) {
      double p1[] = { 2.0, 3.0 }, p2[] = { 7.0, 6.0 };
      reg = (AstRegion *) astBox( f2, 1, p1, p2, NULL, "" );
   } else if( !strcmp( name, "region_or_disjoint" ) ) {
      double c1[] = { 3.0, 3.0 }, c2[] = { 7.0, 7.0 }, r = 1.5;
      AstRegion *a = (AstRegion *) astCircle( f2, 1, c1, &r, NULL, "" );
      AstRegion *b = (AstRegion *) astCircle( f2, 1, c2, &r, NULL, "" );
      reg = (AstRegion *) astCmpRegion( a, b, AST__OR, "" );
      a = astAnnul( a );
      b = astAnnul( b );
   } else if( !strcmp( name, "region_neg_circle" ) ) {
      double c[] = { 5.0, 5.0 }, r = 3.0;
      reg = (AstRegion *) astCircle( f2, 1, c, &r, NULL, "" );
      astNegate( reg );
   } else if( !strcmp( name, "region_prism" ) ) {
      AstFrame *f1 = astFrame( 1, "" );
      double lo1 = 2.0, hi1 = 8.0, lo2 = 3.0, hi2 = 7.0;
      AstRegion *a = (AstRegion *) astInterval( f1, &lo1, &hi1, NULL, "" );
      AstRegion *b = (AstRegion *) astInterval( f1, &lo2, &hi2, NULL, "" );
      reg = (AstRegion *) astPrism( a, b, "" );
      a = astAnnul( a );
      b = astAnnul( b );
      f1 = astAnnul( f1 );
   } else if( !strcmp( name, "region_ellipse" ) ) {
      double c[] = { 5.0, 5.0 }, ab[] = { 3.0, 1.5 }, ang = 0.5;
      reg = (AstRegion *) astEllipse( f2, 1, c, ab, &ang, NULL, "" );
   } else if( !strcmp( name, "region_polygon" ) ) {
      double p[] = { 2.0, 5.0, 8.0, 9.5, 8.0, 5.0, 2.6,
                     2.0, 0.5, 2.0, 5.0, 8.0, 7.4, 5.0 };
      reg = (AstRegion *) astPolygon( f2, 7, 7, p, NULL, "" );
   } else if( !strcmp( name, "region_moc" ) ) {
      const char *text = "3/0-2,4-5,7-8,10-11,13-15";
      int json;
      reg = (AstRegion *) astMoc( "" );
      astAddMocString( (AstMoc *) reg, AST__OR, 0, -1, strlen( text ), text, &json );
      pbox[ 0 ] = 0.3; pbox[ 1 ] = -0.05; pbox[ 2 ] = 1.27; pbox[ 3 ] = 0.8;
   } else if( !strcmp( name, "region_gal_circle" ) ) {
      double c[] = { 1.0, 0.5 }, r = 0.05;
      AstFrame *sky = (AstFrame *) astSkyFrame( "System=ICRS" );
      reg = (AstRegion *) astCircle( sky, 1, c, &r, NULL, "" );
      astSet( reg, "System=Galactic" );
      pbox[ 0 ] = 2.78; pbox[ 1 ] = -0.42; pbox[ 2 ] = 2.94; pbox[ 3 ] = -0.27;
      sky = astAnnul( sky );
   } else {
      fprintf( stderr, "unknown region plot %s\n", name );
      exit( 1 );
   }
   pl = astPlot( reg, gbox, pbox, "" );
   reg = astAnnul( reg );
   f2 = astAnnul( f2 );
   return pl;
}

static const char *region_names[] = {
   "region_circle", "region_sky_circle", "region_box", "region_or_disjoint",
   "region_neg_circle", "region_prism", "region_ellipse", "region_polygon",
   "region_moc", "region_gal_circle",
};
#define NREGION ( (int)( sizeof( region_names ) / sizeof( region_names[ 0 ] ) ) )

static void border_case( const char *name, AstPlot *pl ) {
   int inval;
   printf( "    BorderCase {\n        name: " );
   rstr( name );
   printf( ",\n" );
   capture_start();
   inval = astBorder( pl );
   printf( "        inval: %s,\n", inval ? "true" : "false" );
   capture_end();
   printf( "    },\n" );
   if( !astOK ) {
      fprintf( stderr, "case %s failed with status %d\n", name, astStatus );
      exit( 1 );
   }
}

int main( void ) {
   int status_value = 0;
   struct grid_case cases[ 64 ];
   AstPlot *grid3d = NULL;
   int ncase, i;
   astWatch( &status_value );
   ncase = read_grid_cases( cases, 64 );

   printf( "// Generated by tools/oracle_fixtures/plot_border_probe.c.\n"
           "// Do not edit.\n\n"
           "/// What C's astBorder returned for a Plot, and what grf_log wrote.\n"
           "/// A name is a case of fixtures/plot/cases.txt, one of the 2-D\n"
           "/// Frame Plots `frame`, `frame_log` and `frame_styles`, or\n"
           "/// `car3_3d`, the car3 Plot with a 3-D current Frame: a CmpFrame of\n"
           "/// its sky Frame and a 1-D Frame, joined by a PermMap giving the\n"
           "/// third axis 0, or a Plot whose current Frame is a Region,\n"
           "/// named `region_*`.\n"
           "pub struct BorderCase {\n"
           "    pub name: &'static str,\n"
           "    pub inval: bool,\n"
           "    pub log: &'static str,\n"
           "    pub svg: Svg,\n"
           "}\n\n"
           SVG_DECL
           "pub const CASES: &[BorderCase] = &[\n" );
   for( i = 0; i < ncase; i++ ) {
      struct grid_case *c = cases + i;
      AstPlot *pl = testgrid_plot( c->head, c->fattr, c->has_box ? c->box : NULL,
                                   c->attr );
      border_case( c->name, pl );
      pl = astAnnul( pl );
   }
   for( i = 0; i < ncase; i++ ) {
      struct grid_case *c = cases + i;
      if( !strcmp( c->name, "car3" ) ) {
         AstPlot *pl = testgrid_plot( c->head, c->fattr, c->has_box ? c->box : NULL,
                                      c->attr );
         AstFrame *sky = astGetFrame( pl, AST__CURRENT );
         AstFrame *one = astFrame( 1, "" );
         AstCmpFrame *cube = astCmpFrame( sky, one, "" );
         int inperm[ 2 ] = { 1, 2 };
         int outperm[ 3 ] = { 1, 2, -1 };
         double zero = 0.0;
         AstPermMap *pm = astPermMap( 2, inperm, 3, outperm, &zero, "" );
         astAddFrame( pl, AST__CURRENT, pm, cube );
         border_case( "car3_3d", pl );
         grid3d = astCopy( pl );
         pl = astAnnul( pl );
      }
   }
   {
      float gbox[ 4 ] = { 0.1f, 0.1f, 0.9f, 0.9f };
      double pbox[ 4 ] = { 0.0, 0.0, 10.0, 10.0 };
      double lbox[ 4 ] = { 1.0, 0.0, 1000.0, 10.0 };
      AstFrame *fr = astFrame( 2, "" );
      AstPlot *pl = astPlot( fr, gbox, pbox, "" );
      border_case( "frame", pl );
      pl = astAnnul( pl );
      pl = astPlot( fr, gbox, lbox, "LogPlot(1)=1" );
      border_case( "frame_log", pl );
      pl = astAnnul( pl );
      pl = astPlot( fr, gbox, pbox, "Width(border)=2,Colour(border)=4,Style(border)=3" );
      border_case( "frame_styles", pl );
      pl = astAnnul( pl );
      fr = astAnnul( fr );
   }
   for( i = 0; i < NREGION; i++ ) {
      AstPlot *pl = region_plot( region_names[ i ] );
      border_case( region_names[ i ], pl );
      pl = astAnnul( pl );
   }
   printf( "];\n\n/// astGrid of the car3 Plot with a 3-D current Frame.\n"
           "pub const GRID_3D: (&str, Svg) = (\n" );
   capture_start();
   astGrid( grid3d );
   capture_end_tuple();
   printf( ");\n\n/// `(name, log, svg)` of astGrid for each Plot whose current Frame is\n"
           "/// a Region.\n"
           "pub const REGION_GRIDS: &[(&str, &str, Svg)] = &[\n" );
   for( i = 0; i < NREGION; i++ ) {
      AstPlot *pl = region_plot( region_names[ i ] );
      printf( "    (" );
      rstr( region_names[ i ] );
      printf( ", " );
      capture_start();
      astGrid( pl );
      capture_end_tuple();
      printf( "),\n" );
      pl = astAnnul( pl );
      if( !astOK ) {
         fprintf( stderr, "grid %s failed with status %d\n", region_names[ i ], astStatus );
         exit( 1 );
      }
   }
   printf( "];\n" );
   return astOK ? 0 : 1;
}
