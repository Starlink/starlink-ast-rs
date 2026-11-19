/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* plot_attrs_probe -- capture Plot's own attributes (plot.c Get/Set/Test/
 * Clear for each): their defaults, what setting each stores (C clamps or
 * normalises some values and rejects others), the defaults that follow
 * other attributes, and how setting and clearing LogPlot changes the Plot's
 * Mapping (plot.c ToggleLogLin).
 *
 * Each scenario builds a fresh Plot, applies an astSet string, optionally
 * clears attributes, and records each attribute's value and whether it is
 * set, through the protected accessors of plot.h. Those give the stored value
 * or the fixed default; the public string interface instead gives, for
 * Gap, Centre, MinTick, Grid, Border and the other attributes Plot records
 * while drawing, the value a grid drawn now would use.
 *
 * Prints Rust source for crates/starlink-ast/tests/plot_attrs_oracle/cases.rs:
 *
 *   ./plot_attrs_probe > ../../crates/starlink-ast/tests/plot_attrs_oracle/cases.rs
 *
 * Build and run from reference/starlink-ast/build (configured with
 * -DCMAKE_C_FLAGS=-ffp-contract=off); the car1 frame reads
 * ../ast_tester/fixtures/wcsconv/inputs/car1.head:
 *
 *   cc -ffp-contract=off -O3 -I. ../../../tools/oracle_fixtures/plot_attrs_probe.c \
 *      -L. -last -Wl,-force_load,libast_err.a \
 *      -Wl,-force_load,libast_grf_2.0.a -Wl,-force_load,libast_grf_3.2.a \
 *      -Wl,-force_load,libast_grf_5.6.a -Wl,-force_load,libast_grf3d.a \
 *      -lm -Wl,-rpath,$PWD \
 *      -o ../../../tools/oracle_fixtures/plot_attrs_probe
 *
 * On Linux, link the error and graphics stub libraries with
 * -Wl,--whole-archive -last_err -last_grf_2.0 -last_grf_3.2 -last_grf_5.6
 * -last_grf3d -Wl,--no-whole-archive.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"

static void flit( double value ) {
   char buf[ 40 ];
   if( value == AST__BAD ) { fputs( "BAD", stdout ); return; }
   snprintf( buf, sizeof( buf ), "%.17g", value );
   fputs( buf, stdout );
   if( !strpbrk( buf, ".en" ) ) fputs( ".0", stdout );
}

static void rstr( const char *s ) {
   putchar( '"' );
   for( ; *s; s++ ) {
      if( *s == '"' || *s == '\\' ) putchar( '\\' );
      putchar( *s );
   }
   putchar( '"' );
}

static AstFrameSet *read_head( const char *path ) {
   char line[ 200 ];
   FILE *fp = fopen( path, "r" );
   AstFitsChan *fc = astFitsChan( NULL, NULL, " " );
   AstFrameSet *fs;
   if( !fp ) { fprintf( stderr, "cannot open %s\n", path ); exit( 1 ); }
   while( fgets( line, sizeof( line ), fp ) ) {
      line[ strcspn( line, "\n" ) ] = 0;
      astPutFits( fc, line, 0 );
   }
   fclose( fp );
   astClear( fc, "Card" );
   fs = astRead( fc );
   fc = astAnnul( fc );
   return fs;
}

/* The Plot a frame id names. */
static AstPlot *make_plot( const char *id ) {
   float g[ 4 ] = { 0.1f, 0.1f, 0.9f, 0.9f };
   double b[ 4 ] = { 0.0, 0.0, 100.0, 50.0 };
   AstObject *f = NULL;
   AstPlot *p;
   if( !strcmp( id, "basic2" ) ) {
      f = (AstObject *) astFrame( 2, " " );
   } else if( !strcmp( id, "basic2_pos" ) ) {
      f = (AstObject *) astFrame( 2, " " );
      b[ 0 ] = 1.0; b[ 1 ] = 2.0;
   } else if( !strcmp( id, "basic2_neg" ) ) {
      f = (AstObject *) astFrame( 2, " " );
      b[ 0 ] = -100.0; b[ 1 ] = -50.0; b[ 2 ] = -1.0; b[ 3 ] = -2.0;
   } else if( !strcmp( id, "basic2_rev" ) ) {
      f = (AstObject *) astFrame( 2, " " );
      b[ 0 ] = 1.0; b[ 1 ] = 2.0;
      g[ 0 ] = 0.9f; g[ 2 ] = 0.1f;
   } else if( !strcmp( id, "sky" ) ) {
      f = (AstObject *) astSkyFrame( " " );
      b[ 0 ] = 0.0; b[ 1 ] = -0.5; b[ 2 ] = 1.0; b[ 3 ] = 0.5;
   } else if( !strcmp( id, "sky_gal" ) ) {
      f = (AstObject *) astSkyFrame( "System=Galactic" );
      b[ 0 ] = 0.0; b[ 1 ] = -0.5; b[ 2 ] = 1.0; b[ 3 ] = 0.5;
   } else if( !strcmp( id, "sky_azel" ) ) {
      f = (AstObject *) astSkyFrame( "System=AzEl" );
      b[ 0 ] = 0.0; b[ 1 ] = -0.5; b[ 2 ] = 1.0; b[ 3 ] = 0.5;
   } else if( !strcmp( id, "sky_spec" ) ) {
      AstFrame *s = (AstFrame *) astSpecFrame( " " );
      AstFrame *k = (AstFrame *) astSkyFrame( " " );
      f = (AstObject *) astCmpFrame( s, k, " " );
      s = astAnnul( s ); k = astAnnul( k );
      b[ 0 ] = 1.0; b[ 1 ] = 1.0; b[ 2 ] = 2.0; b[ 3 ] = 2.0;
   } else if( !strcmp( id, "car1" ) ) {
      f = (AstObject *) read_head( "../ast_tester/fixtures/wcsconv/inputs/car1.head" );
      b[ 0 ] = 0.0; b[ 1 ] = 0.0; b[ 2 ] = 2962.0; b[ 3 ] = 562.0;
   } else {
      fprintf( stderr, "unknown frame id %s\n", id );
      exit( 1 );
   }
   p = astPlot( f, g, b, " " );
   f = astAnnul( f );
   return p;
}

/* The protected accessors (plot.h), called with real object pointers.
   These give an attribute's stored value or its fixed default, which is
   what a Plot holds and dumps; the public string interface instead returns,
   for some attributes, the value a grid drawn now would use. */
#define SCALAR( a, t ) extern t astGet##a##_( void *, int * ); \
                       extern int astTest##a##_( void *, int * );
#define AXIS( a, t ) extern t astGet##a##_( void *, int, int * ); \
                     extern int astTest##a##_( void *, int, int * );
SCALAR( Tol, double ) SCALAR( Grid, int ) SCALAR( TickAll, int )
SCALAR( ForceExterior, int ) SCALAR( Invisible, int ) SCALAR( Border, int )
SCALAR( ClipOp, int ) SCALAR( Clip, int ) SCALAR( DrawTitle, int )
SCALAR( Escape, int ) SCALAR( TitleGap, double ) SCALAR( Labelling, int )
SCALAR( TextGapType, int )
AXIS( DrawAxes, int ) AXIS( Abbrev, int ) AXIS( LabelAt, double )
AXIS( Centre, double ) AXIS( Gap, double ) AXIS( LogGap, double )
AXIS( NumLabGap, double ) AXIS( TextLabGap, double ) AXIS( LabelUp, int )
AXIS( LogPlot, int ) AXIS( LogTicks, int ) AXIS( LogLabel, int )
AXIS( NumLab, int ) AXIS( MinTick, int ) AXIS( TextLab, int )
AXIS( LabelUnits, int ) AXIS( MajTickLen, double ) AXIS( MinTickLen, double )
AXIS( Edge, int )

static const char *xedge[ 4 ] = { "left", "top", "right", "bottom" };
static const char *xlbling[ 2 ] = { "exterior", "interior" };
static const char *xtgaptype[ 2 ] = { "box", "plot" };

/* Attribute kinds: 'i' integer, 'd' double, and the string kinds 'e'
   (Edge), 'l' (Labelling) and 't' (TextGapType), held as integers. */
struct attr {
   const char *name; char kind;
   int (*geti)( void *, int * ); double (*getd)( void *, int * );
   int (*test)( void *, int * );
   int (*ageti)( void *, int, int * ); double (*agetd)( void *, int, int * );
   int (*atest)( void *, int, int * );
};
#define SI( a ) { #a, 'i', astGet##a##_, NULL, astTest##a##_, NULL, NULL, NULL }
#define SD( a ) { #a, 'd', NULL, astGet##a##_, astTest##a##_, NULL, NULL, NULL }
#define SS( a, k ) { #a, k, astGet##a##_, NULL, astTest##a##_, NULL, NULL, NULL }
#define AI( a ) { #a, 'i', NULL, NULL, NULL, astGet##a##_, NULL, astTest##a##_ }
#define AD( a ) { #a, 'd', NULL, NULL, NULL, NULL, astGet##a##_, astTest##a##_ }
#define AS( a, k ) { #a, k, NULL, NULL, NULL, astGet##a##_, NULL, astTest##a##_ }
static const struct attr scalars[] = {
   SD( Tol ), SI( Grid ), SI( TickAll ), SI( ForceExterior ), SI( Invisible ),
   SI( Border ), SI( ClipOp ), SI( Clip ), SI( DrawTitle ), SI( Escape ),
   SD( TitleGap ), SS( Labelling, 'l' ), SS( TextGapType, 't' ),
};
static const struct attr peraxis[] = {
   AI( DrawAxes ), AI( Abbrev ), AD( LabelAt ), AD( Centre ), AD( Gap ),
   AD( LogGap ), AD( NumLabGap ), AD( TextLabGap ), AI( LabelUp ),
   AI( LogPlot ), AI( LogTicks ), AI( LogLabel ), AI( NumLab ), AI( MinTick ),
   AI( TextLab ), AI( LabelUnits ), AD( MajTickLen ), AD( MinTickLen ),
   AS( Edge, 'e' ),
};
#define NUMOF(a) ( (int)( sizeof( a ) / sizeof( ( a )[ 0 ] ) ) )

/* A scenario: a Plot, an astSet string, and an astClear string, after which
   every attribute is queried. */
struct scenario { const char *id; const char *set; const char *clear; };
static const struct scenario scenarios[] = {
   { "basic2", "", "" },
   { "sky", "", "" },
   { "sky_gal", "", "" },
   { "sky_azel", "", "" },
   { "sky_spec", "", "" },
   { "car1", "", "" },
   { "basic2", "Tol=0.5,Grid=1,TickAll=0,ForceExterior=1,Invisible=1,Border=0,"
     "ClipOp=1,Clip=2,DrawTitle=0,Escape=0,TitleGap=0.1,"
     "Labelling=interior,TextGapType=plot", "" },
   { "basic2", "Tol=2.0", "" },
   { "basic2", "Tol=1.0E-9", "" },
   { "basic2", "Clip=3", "" },
   { "basic2", "Grid=7,TickAll=-2", "" },
   { "basic2", "DrawAxes(1)=0,Abbrev(2)=0,LabelAt(1)=12.5,Centre(2)=3.0,"
     "Gap(1)=10.0,LogGap(2)=100.0,NumLabGap(1)=0.02,TextLabGap(2)=0.03,"
     "LabelUp(1)=1,NumLab(2)=0,MinTick(1)=4,TextLab(2)=0,LabelUnits(1)=0,"
     "MajTickLen(1)=0.02,MinTickLen(2)=-0.01,Edge(1)=top,Edge(2)=right", "" },
   { "basic2", "MinTick(1)=0,MinTick(2)=-3", "" },
   { "basic2", "Edge(1)=b,Edge(2)=l", "" },
   { "basic2", "TextGapType=plot", "" },
   { "basic2", "Grid=1,Gap(1)=10.0,Edge(2)=right,LogPlot(1)=1",
     "Grid,Gap(1),Edge(2)" },
   /* LogPlot, and the defaults that follow it. */
   { "basic2", "LogPlot(1)=1", "" },
   { "basic2_pos", "LogPlot(1)=1", "" },
   { "basic2_pos", "LogPlot(2)=1,LogTicks(2)=0", "" },
   { "basic2_pos", "LogPlot(1)=1", "LogPlot(1)" },
   { "basic2_pos", "LogPlot(1)=0", "" },
   { "basic2_neg", "LogPlot(1)=1,LogPlot(2)=1", "" },
   { "basic2_rev", "LogPlot(1)=1", "" },
   { "basic2_pos", "LogTicks(1)=1", "" },
   { "basic2_pos", "LogLabel(2)=1", "" },
};
#define NSCEN NUMOF( scenarios )

/* One attribute, through the protected accessors: whether it is set, and
   its value. `axis` is 1-based, or 0 for a scalar attribute. */
static void query( AstPlot *p, const struct attr *a, int axis, int *status ) {
   void *ptr = astMakePointer( p );
   char name[ 40 ];
   int set, iv = 0;
   double dv = 0.0;
   if( axis ) {
      snprintf( name, sizeof( name ), "%s(%d)", a->name, axis );
      set = a->atest( ptr, axis - 1, status );
      if( a->agetd ) dv = a->agetd( ptr, axis - 1, status );
      else iv = a->ageti( ptr, axis - 1, status );
   } else {
      snprintf( name, sizeof( name ), "%s", a->name );
      set = a->test( ptr, status );
      if( a->getd ) dv = a->getd( ptr, status );
      else iv = a->geti( ptr, status );
   }
   printf( "        (" ); rstr( name ); printf( ", Some(%s), ", set ? "true" : "false" );
   switch( a->kind ) {
   case 'i': printf( "Value::I(%d)", iv ); break;
   case 'd': printf( "Value::D(" ); flit( dv ); printf( ")" ); break;
   case 'e': printf( "Value::S(" ); rstr( xedge[ iv ] ); printf( ")" ); break;
   case 'l': printf( "Value::S(" ); rstr( xlbling[ iv ] ); printf( ")" ); break;
   case 't': printf( "Value::S(" ); rstr( xtgaptype[ iv ] ); printf( ")" ); break;
   }
   printf( "),\n" );
}

/* The graphical elements, in the order of their ids (plot.c GrfLabels),
   and the per-element style attributes. */
static const char *elements[] = {
   "Border", "Curves", "Title", "Markers", "Strings", "Axis1", "Axis2", "Axis3",
   "NumLab1", "NumLab2", "NumLab3", "TextLab1", "TextLab2", "TextLab3",
   "Ticks1", "Ticks2", "Ticks3", "Grid1", "Grid2", "Grid3", "Axes", "NumLab",
   "TextLab", "Grid", "Ticks",
};
static const struct attr styles[] = {
   { "Style", 'i' }, { "Font", 'i' }, { "Colour", 'i' }, { "Width", 'd' },
   { "Size", 'd' },
};

/* Plot style scenarios: an astSet string and an astClear string on a Plot
   of a 2-D Frame, then every style attribute of every element, read through
   the string interface, which is where element groups are resolved. */
static const char *style_sets[][ 2 ] = {
   { "", "" },
   { "Colour(Axes)=3", "" },
   { "Colour(Axes)=3", "Colour(Axes)" },
   { "Colour(Axis2)=4", "" },
   { "Width(Grid1)=2.5,Size(Strings)=0.8,Style(Ticks2)=4,Font(Title)=3", "" },
   { "Font(TextLab)=2,Font(TextLab1)=5", "" },
   { "Style(Ticks)=-1", "" },
   { "Size(Markers)=-1.5,Width(Border)=0.25", "" },
   { "Width(Border)=0", "" },
   { "Size(Title)=0", "" },
   { "Colour(NumLab)=6,Width(GridLine)=2", "" },
   { "Colour(Axis3)=7,Width(Ticks3)=3", "" },
};

static void style_scenarios( int *status ) {
   int s, e, a;
   printf( "/// A Plot of a 2-D Frame with an astSet and an astClear string, then\n"
           "/// each style attribute of each element, as `(attribute, element,\n"
           "/// set, value)`; `None` when C rejected the setting.\n"
           "pub struct StyleScenario {\n"
           "    pub set: &'static str,\n"
           "    pub clear: &'static str,\n"
           "    pub attrs: Option<&'static [(&'static str, &'static str, bool, Value)]>,\n"
           "}\n\n"
           "pub const STYLE_SCENARIOS: &[StyleScenario] = &[\n" );
   for( s = 0; s < NUMOF( style_sets ); s++ ) {
      AstPlot *p = make_plot( "basic2" );
      printf( "    StyleScenario {\n        set: " ); rstr( style_sets[ s ][ 0 ] );
      printf( ",\n        clear: " ); rstr( style_sets[ s ][ 1 ] ); printf( ",\n" );
      if( style_sets[ s ][ 0 ][ 0 ] ) astSet( p, "%s", style_sets[ s ][ 0 ] );
      if( astOK && style_sets[ s ][ 1 ][ 0 ] ) astClear( p, style_sets[ s ][ 1 ] );
      if( !astOK ) {
         astClearStatus;
         printf( "        attrs: None,\n    },\n" );
         p = astAnnul( p );
         continue;
      }
      printf( "        attrs: Some(&[\n" );
      for( a = 0; a < NUMOF( styles ); a++ ) {
         for( e = 0; e < NUMOF( elements ); e++ ) {
            char name[ 40 ];
            int set;
            snprintf( name, sizeof( name ), "%s(%s)", styles[ a ].name, elements[ e ] );
            set = astTest( p, name );
            printf( "            (\"%s\", \"%s\", %s, ", styles[ a ].name, elements[ e ],
                    set ? "true" : "false" );
            if( styles[ a ].kind == 'i' ) {
               printf( "Value::I(%d)", astGetI( p, name ) );
            } else {
               printf( "Value::D(" ); flit( astGetD( p, name ) ); printf( ")" );
            }
            printf( "),\n" );
         }
      }
      printf( "        ]),\n    },\n" );
      p = astAnnul( p );
   }
   printf( "];\n" );
}

int main( void ) {
   int status = 0, s, i, ax;
   double gx[ 3 ] = { 0.15, 0.5, 0.85 }, gy[ 3 ] = { 0.2, 0.5, 0.8 };
   astWatch( &status );

   printf( "// Generated by tools/oracle_fixtures/plot_attrs_probe.c.\n"
           "// Do not edit.\n\n"
           "#![allow(clippy::excessive_precision, clippy::approx_constant)]\n\n"
           "use starlink_ast::constants::BAD_VALUE as BAD;\n\n"
           "/// An attribute value as C reads it.\n"
           "#[derive(Debug, Clone, PartialEq)]\n"
           "pub enum Value {\n    I(i32),\n    D(f64),\n    S(&'static str),\n    Err,\n}\n\n"
           "/// A Plot of a Frame id, an astSet string, an astClear string, and\n"
           "/// then each attribute's name, whether it is set, and its value; or\n"
           "/// `None` when C rejected the setting.\n"
           "pub struct Scenario {\n"
           "    pub id: &'static str,\n"
           "    pub set: &'static str,\n"
           "    pub clear: &'static str,\n"
           "    pub attrs: Option<&'static [(&'static str, Option<bool>, Value)]>,\n"
           "    /// Graphics points (0.15, 0.2), (0.5, 0.5), (0.85, 0.8)\n"
           "    /// transformed forward, for a Plot with two outputs.\n"
           "    pub forward: &'static [[f64; 2]],\n"
           "}\n\n"
           "pub const SCENARIOS: &[Scenario] = &[\n" );

   for( s = 0; s < NSCEN; s++ ) {
      const struct scenario *c = scenarios + s;
      AstPlot *p = make_plot( c->id );
      printf( "    Scenario {\n        id: " ); rstr( c->id );
      printf( ",\n        set: " ); rstr( c->set );
      printf( ",\n        clear: " ); rstr( c->clear ); printf( ",\n" );
      if( c->set[ 0 ] ) astSet( p, "%s", c->set );
      if( astOK && c->clear[ 0 ] ) astClear( p, c->clear );
      if( !astOK ) {
         astClearStatus;
         printf( "        attrs: None,\n        forward: &[],\n    },\n" );
         p = astAnnul( p );
         continue;
      }
      printf( "        attrs: Some(&[\n" );
      for( i = 0; i < NUMOF( scalars ); i++ ) {
         printf( "    " ); query( p, scalars + i, 0, &status );
      }
      for( i = 0; i < NUMOF( peraxis ); i++ ) {
         for( ax = 1; ax <= 2; ax++ ) {
            printf( "    " ); query( p, peraxis + i, ax, &status );
         }
      }
      printf( "        ]),\n        forward: &[" );
      for( i = 0; i < 3 && astGetI( p, "Nout" ) == 2; i++ ) {
         double x = gx[ i ], y = gy[ i ], ox, oy;
         astTran2( p, 1, &x, &y, 1, &ox, &oy );
         printf( "%s[", i ? ", " : "" ); flit( ox ); printf( ", " ); flit( oy );
         printf( "]" );
      }
      printf( "],\n    },\n" );
      p = astAnnul( p );
   }
   printf( "];\n\n" );
   style_scenarios( &status );
   return astOK ? 0 : 1;
}
