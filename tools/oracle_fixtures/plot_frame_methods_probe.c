/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* plot_frame_methods_probe -- capture what astGap, astCentre, astAbbrev and
 * astFields return, the Frame methods Plot uses to choose tick spacing and
 * to split and abbreviate axis labels (frame.c Gap, Centre, Abbrev, Fields,
 * and the per-class overrides they reach).
 *
 * Each case names a Frame by an id that the Rust test builds the same way.
 *
 * Prints Rust source for
 * crates/starlink-ast/tests/plot_frame_methods_oracle/cases.rs:
 *
 *   ./plot_frame_methods_probe > ../../crates/starlink-ast/tests/plot_frame_methods_oracle/cases.rs
 *
 * Build from reference/starlink-ast/build (configured with
 * -DCMAKE_C_FLAGS=-ffp-contract=off):
 *
 *   cc ../../../tools/oracle_fixtures/plot_frame_methods_probe.c -I. -L. -last -lm \
 *      -Wl,-rpath,$PWD -o ../../../tools/oracle_fixtures/plot_frame_methods_probe
 */
#include <float.h>
#include <stdio.h>
#include <string.h>
#include "ast.h"

/* The methods are protected (frame.h); call their entry points with real
   object pointers. Axis indices are zero-based. */
extern const char *astAbbrev_( void *, int, const char *, const char *,
                               const char *, int * );
extern double astCentre_( void *, int, double, double, int * );
extern double astGap_( void *, int, double, int *, int * );
extern int astFields_( void *, int, const char *, const char *, int, char **,
                       int *, double *, int * );

#define MAXFLD 20

static void flit( double value ) {
   char buf[ 40 ];
   if( value == AST__BAD ) { fputs( "BAD", stdout ); return; }
   if( value > DBL_MAX ) { fputs( "f64::INFINITY", stdout ); return; }
   if( value < -DBL_MAX ) { fputs( "f64::NEG_INFINITY", stdout ); return; }
   snprintf( buf, sizeof( buf ), "%.17g", value );
   fputs( buf, stdout );
   if( !strpbrk( buf, ".en" ) ) fputs( ".0", stdout );
}

/* A Rust string literal. */
static void rstr( const char *s ) {
   putchar( '"' );
   for( ; *s; s++ ) {
      if( *s == '"' || *s == '\\' ) putchar( '\\' );
      putchar( *s );
   }
   putchar( '"' );
}

/* The Frame a case id names. */
static AstFrame *make_frame( const char *id ) {
   if( !strcmp( id, "basic2" ) ) return astFrame( 2, " " );
   if( !strcmp( id, "sky" ) ) return (AstFrame *) astSkyFrame( " " );
   if( !strcmp( id, "sky_perm" ) ) {
      int perm[ 2 ] = { 2, 1 };
      AstFrame *f = (AstFrame *) astSkyFrame( " " );
      astPermAxes( f, perm );
      return f;
   }
   if( !strcmp( id, "sky_fmt" ) ) {
      return (AstFrame *) astSkyFrame( "Format(1)=hms.2,Format(2)=dms.1" );
   }
   if( !strcmp( id, "sky_digits" ) ) return (AstFrame *) astSkyFrame( "Digits=10" );
   if( !strcmp( id, "sky_axdigits" ) ) return (AstFrame *) astSkyFrame( "Digits(1)=3" );
   if( !strcmp( id, "sky_gal" ) ) return (AstFrame *) astSkyFrame( "System=Galactic" );
   if( !strcmp( id, "time" ) ) return (AstFrame *) astTimeFrame( " " );
   if( !strcmp( id, "time_iso" ) ) return (AstFrame *) astTimeFrame( "Format=iso.0" );
   if( !strcmp( id, "time_iso2" ) ) return (AstFrame *) astTimeFrame( "Format=iso.2" );
   if( !strcmp( id, "time_isodate" ) ) return (AstFrame *) astTimeFrame( "Format=iso" );
   if( !strcmp( id, "time_jd_iso" ) ) {
      return (AstFrame *) astTimeFrame( "System=JD,Unit=d,Format=iso.0" );
   }
   if( !strcmp( id, "time_origin_iso" ) ) {
      return (AstFrame *) astTimeFrame( "TimeOrigin=56000,Format=iso.0" );
   }
   if( !strcmp( id, "time_unit_h_iso" ) ) {
      return (AstFrame *) astTimeFrame( "Unit=h,Format=iso.0" );
   }
   if( !strcmp( id, "cmp_sky_time" ) || !strcmp( id, "cmp_perm" ) ) {
      AstFrame *sky = (AstFrame *) astSkyFrame( " " );
      AstFrame *time = (AstFrame *) astTimeFrame( "Format=iso.0" );
      AstFrame *f = (AstFrame *) astCmpFrame( sky, time, "Digits=10" );
      if( !strcmp( id, "cmp_perm" ) ) {
         int perm[ 3 ] = { 3, 1, 2 };
         astPermAxes( f, perm );
      }
      sky = astAnnul( sky );
      time = astAnnul( time );
      return f;
   }
   if( !strcmp( id, "specflux" ) ) {
      AstFrame *spec = (AstFrame *) astSpecFrame( " " );
      AstFrame *flux = (AstFrame *) astFluxFrame( AST__BAD, NULL, " " );
      AstFrame *f = (AstFrame *) astSpecFluxFrame( (AstSpecFrame *) spec,
                                                   (AstFluxFrame *) flux, " " );
      spec = astAnnul( spec );
      flux = astAnnul( flux );
      return f;
   }
   if( !strcmp( id, "frameset_sky" ) ) {
      AstFrame *base = astFrame( 2, " " );
      AstFrame *sky = make_frame( "sky_fmt" );
      AstUnitMap *um = astUnitMap( 2, " " );
      AstFrameSet *fs = astFrameSet( base, " " );
      astAddFrame( fs, AST__BASE, um, sky );
      base = astAnnul( base );
      sky = astAnnul( sky );
      um = astAnnul( um );
      return (AstFrame *) fs;
   }
   if( !strcmp( id, "box_time_iso" ) ) {
      AstFrame *time = make_frame( "time_iso" );
      double lo[ 1 ] = { 56000.0 }, hi[ 1 ] = { 56001.0 };
      AstFrame *f = (AstFrame *) astBox( time, 1, lo, hi, NULL, " " );
      time = astAnnul( time );
      return f;
   }
   if( !strcmp( id, "box_sky" ) ) {
      AstFrame *sky = make_frame( "sky" );
      double lo[ 2 ] = { 0.0, 0.0 }, hi[ 2 ] = { 0.1, 0.1 };
      AstFrame *f = (AstFrame *) astBox( sky, 1, lo, hi, NULL, " " );
      sky = astAnnul( sky );
      return f;
   }
   if( !strcmp( id, "basic_skyaxis_ra" ) || !strcmp( id, "basic_skyaxis_dec" ) ) {
      int axes[ 1 ];
      AstFrame *sky = (AstFrame *) astSkyFrame( " " ), *f;
      axes[ 0 ] = strcmp( id, "basic_skyaxis_ra" ) ? 2 : 1;
      f = astPickAxes( sky, 1, axes, NULL );
      sky = astAnnul( sky );
      return f;
   }
   printf( "unknown frame id %s\n", id );
   return NULL;
}

/* Gap sizes tried on every axis of every Frame. */
static const double gaps[] = {
   0.0, AST__BAD, 1.0E-300, -1.0E-300, 1.0E-9, 0.0037, -0.0037, 1.0, -1.0,
   1.4, 1.6, 2.4, 2.6, 3.4, 3.6, 4.4, 4.6, 5.4, 5.6, 6.4, 6.6, 7.4, 7.6,
   8.4, 8.6, 9.4, 9.6, -9.6, 0.25, 0.5, 0.75, 1.5, 7.4, 123.0, 1.0E300,
   1.0E-7, 3.0E-6, 5.0E-6, 2.0E-5, 6.0E-5, 1.0E-4, 3.0E-4, 6.0E-4, 1.0E-3,
   3.0E-3, 6.0E-3, 0.01, 0.02, 0.05, 0.1, 0.2, 0.3, 0.8, 2.0, 3.0, 6.0,
   -0.3, 7.0, 100.0
};

/* (value, gap) pairs tried for astCentre on every axis of every Frame. */
static const double centres[][ 2 ] = {
   { AST__BAD, 1.0 }, { 17.3, 0.0 }, { 17.3, AST__BAD }, { 17.3, 5.0 },
   { -2.5, 5.0 }, { 2.5, 5.0 }, { -17.3, 5.0 }, { 0.3, 0.25 },
   { 1.2, 0.1 }, { 4.0, -3.0 }
};

/* Gap sizes tried on TimeFrames, kept away from the thresholds of
   timeframe.c Gap's ladder: with a date Format the gap is measured from the
   current time, so the rounding of a value at a threshold depends on the
   clock. */
static const double time_gaps[] = {
   0.0, 1.0E-7, 1.0E-5, 3.0E-5, 6.0E-5, 1.2E-4, 2.5E-4, 5.0E-4, 1.0E-3,
   2.5E-3, 5.0E-3, 0.01, 0.02, 0.03, 0.06, 0.1, 0.2, 0.3, 0.6, 1.0, 1.7,
   2.5, 3.7, 5.2, 8.0, 13.0, 20.0, 25.0, 50.0, 100.0, 200.0, 300.0, 400.0,
   1000.0, 5000.0, -1.0, -0.3
};

struct time_centre_case { const char *id; double value; double gap; };
/* A date Format makes C format the value a gap either side; a bad value, or
   a gap too small to change the formatted text, sends timeframe.c Centre
   past the end of its strings, and a gap that changes the month makes it
   read a date with month zero, for which astReadDateTime returns an
   uninitialized value. None of these appears here. */
static const struct time_centre_case time_centres[] = {
   { "time", 56000.37, 0.1 }, { "time", 56000.37, 0.0 },
   { "time_iso", 56000.37, 0.001 }, { "time_iso", 56000.37, 0.1 },
   { "time_iso", 56000.37, 1.0 }, { "time_iso", 56000.37, -1.0 },
   { "time_iso2", 56000.37, 1.0E-4 }, { "time_isodate", 56000.37, 3.0 },
   { "time_jd_iso", 2456000.87, 1.0 }, { "time_origin_iso", 0.37, 1.0 },
   { "time_unit_h_iso", 1344008.88, 2.0 },
   { "box_time_iso", 56000.37, 1.0 }, { "cmp_perm", 56000.37, 1.0 },
};

/* TimeFrames inside compound Frames, as (frame id, axis): their gap is
   measured from the current time as the TimeFrame's own is. */
static const struct { const char *id; int axis; } compound_time_axes[] = {
   { "cmp_sky_time", 3 }, { "cmp_perm", 1 }, { "box_time_iso", 1 },
};

struct axis_case { const char *id; int axis; };
static const struct axis_case time_axes[] = {
   { "time", 1 }, { "time_iso", 1 }, { "time_iso2", 1 }, { "time_isodate", 1 },
   { "time_jd_iso", 1 }, { "time_origin_iso", 1 }, { "time_unit_h_iso", 1 },
};
static const struct axis_case axes[] = {
   { "basic2", 1 }, { "basic2", 2 },
   { "sky", 1 }, { "sky", 2 }, { "sky_perm", 1 }, { "sky_perm", 2 },
   { "sky_fmt", 1 }, { "sky_fmt", 2 }, { "sky_digits", 1 },
   { "sky_digits", 2 }, { "sky_axdigits", 1 }, { "sky_axdigits", 2 },
   { "sky_gal", 1 }, { "sky_gal", 2 }, { "basic_skyaxis_ra", 1 },
   { "basic_skyaxis_dec", 1 },
   { "cmp_sky_time", 1 }, { "cmp_sky_time", 2 },
   { "cmp_perm", 2 }, { "cmp_perm", 3 }, { "specflux", 1 }, { "specflux", 2 },
   { "frameset_sky", 1 }, { "frameset_sky", 2 }, { "box_sky", 1 },
   { "box_sky", 2 },
};

struct abbrev_case { const char *id; int axis; const char *fmt;
                     const char *str1; const char *str2; };
static const struct abbrev_case abbrevs[] = {
   { "basic2", 1, "%g", "1.5", "1.5" },
   { "basic2", 1, "%g", "", "12" },
   { "basic2", 1, "%g", "1.5", "2.5" },
   { "basic2", 2, "%g", "", "" },
   { "basic2", 2, "%&g", "10^4", "10^4" },
   /* A sky axis value with fewer separators than its Format names makes C
      dereference a null pointer (skyaxis.c AxisFields), so every sky string
      here is complete. */
   { "sky", 1, "hms", "12:34:56", "12:34:57" },
   { "sky", 1, "hms", "12:34:56", "12:35:00" },
   { "sky", 1, "hms", "12:34:56", "13:00:00" },
   { "sky", 1, "hms", "12:34:56", "12:34:56" },
   { "sky", 1, "hms", "12:34:56", "a:b:c" },
   { "sky", 1, "lhms", "12h34m56s", "12h34m58s" },
   { "sky", 1, "lhms", "12h34m56s", "12h34m56s" },
   { "sky", 2, "dms", "-01:02:03", "-01:02:04" },
   { "sky", 2, "dms", "01:02:03", "-01:02:03" },
   { "sky", 2, "dm", "-01:02", "-01:03" },
   { "sky", 2, "d", "-01.5", "-01.5" },
   { "sky", 1, "%g", "1.5", "1.5" },
   { "sky_perm", 1, "dms", "10:20:30", "10:20:31" },
   { "basic_skyaxis_dec", 1, "dms", "-01:02:03", "-01:02:09" },
   { "basic_skyaxis_ra", 1, "hms.1", "01:02:03.4", "01:02:03.5" },
   { "time_iso", 1, "iso.0", "2012-03-14 08:52:48", "2012-03-14 08:53:00" },
   { "time_iso", 1, "iso.0", "2012-03-14 08:52:48", "2012-03-15 00:00:00" },
   { "time_iso", 1, "iso.0", "2012-03-14 08:52:48", "2012-03-14 08:52:48" },
   { "time_iso", 1, "iso.0", "2012-03-14 08:52:48", "2013-03-14 08:52:48" },
   { "time_iso", 1, "iso.2", "2012-03-14 08:52:48.25", "2012-03-14 08:52:48.50" },
   { "time_iso", 1, "iso.0", "  2012-03-14", "2012-03-14 08:00:00" },
   { "time_iso", 1, "iso.0T", "2012-03-14T08:52:48", "2012-03-14T09:00:00" },
   { "time_iso", 1, "%g", "1.5", "1.5" },
   { "time", 1, "%1.7G", "56000.5", "56000.5" },
   { "cmp_sky_time", 1, "hms", "12:34:56", "12:34:57" },
   { "cmp_sky_time", 3, "iso.0", "2012-03-14 08:52:48", "2012-03-14 08:53:00" },
   { "cmp_perm", 1, "iso.0", "2012-03-14 08:52:48", "2012-03-14 08:53:00" },
   { "cmp_perm", 3, "dms", "-01:02:03", "-01:02:04" },
   { "specflux", 1, "%g", "1.5", "1.5" },
   { "frameset_sky", 1, "hms", "12:34:56", "12:35:00" },
   { "box_time_iso", 1, "iso.0", "2012-03-14 08:52:48", "2012-03-14 08:53:00" },
   { "box_sky", 2, "dms", "-01:02:03", "-01:02:04" },
};

struct fields_case { const char *id; int axis; const char *fmt;
                     const char *text; int maxfld; };
static const struct fields_case fieldss[] = {
   { "basic2", 1, "%g", "1.5e3", 20 },
   { "basic2", 1, "%g", "  -7 ", 20 },
   { "basic2", 1, "%g", "  -7 ", 1 },
   { "basic2", 1, "%g", "  -7 ", 0 },
   { "basic2", 1, "%g", "", 20 },
   { "basic2", 1, "%g", "1.5 x", 20 },
   { "basic2", 1, "%g", "<bad>", 20 },
   { "basic2", 2, "%&g", "10^4", 20 },
   { "basic2", 2, "%&g", "10^4", 1 },
   { "basic2", 2, "%&g", " -10^-2.5", 20 },
   { "basic2", 2, "%&g", "+10^3", 20 },
   { "basic2", 2, "%&g", "10^<bad>", 20 },
   { "basic2", 2, "%&g", "0", 20 },
   { "basic2", 2, "%&g", "  0.0  ", 20 },
   { "basic2", 2, "%&g", "7", 20 },
   { "basic2", 2, "%&g", "10^x", 20 },
   { "basic2", 2, "%+&g", "10^ 4 tail", 20 },
   { "sky", 1, "hms", "12:34:56", 3 },
   { "sky", 1, "hms", "12:34:56", 1 },
   { "sky", 1, "hms", "12:34:56", 0 },
   { "sky", 1, "hms.2", "12:34:56.78", 3 },
   { "sky", 1, "lhms", "12h34m56s", 3 },
   { "sky", 1, "hms", "12:34:56 x", 3 },
   { "sky", 1, "hms", "a:b:c", 3 },
   { "sky", 1, "%g", "1.5", 3 },
   { "sky", 1, "ms", "34:56", 3 },
   { "sky", 1, "hm", "12:34", 3 },
   { "sky", 1, "h", "12.5", 3 },
   { "sky", 2, "dms", "-01:02:03", 3 },
   { "sky", 2, "+zdms", "+01:02:03", 3 },
   { "sky", 2, "bdms", "-1 02 03", 3 },
   { "sky", 2, "ldms", "-1d02m03s", 3 },
   { "sky", 2, "d", "-12.5", 3 },
   { "sky", 2, "d", "+12.5 ", 3 },
   { "sky", 2, "d", "  -12.5  ", 3 },
   { "sky", 2, "dm", "12:", 3 },
   { "sky", 2, "dms.1", "  -00:00:01.5", 3 },
   { "sky", 2, "dms.*", "-00:00:01.5", 3 },
   { "sky_perm", 1, "dms", "10:20:30", 3 },
   { "sky_axdigits", 1, "hms.*", "12:34:56.7", 3 },
   { "basic_skyaxis_dec", 1, "dms", "-01:02:03", 3 },
   { "basic_skyaxis_ra", 1, "hms", "01:02:03", 3 },
   { "time_iso", 1, "iso.0", "2012-03-14 08:52:48", 20 },
   { "time_iso", 1, "iso.0", "2012-03-14 08:52:48", 3 },
   { "time_iso", 1, "iso.0", "2012-03-14x", 20 },
   { "time_iso", 1, "iso", "2012-03-14", 20 },
   { "time_iso", 1, "iso.0", "2012-03-14 08:52:48  ", 20 },
   { "time_iso2", 1, "iso.2", "2012-03-14T08:52:48.25", 20 },
   { "time_iso", 1, "%g", "56000.5", 20 },
   { "time", 1, "%1.7G", "56000.5", 20 },
   { "time", 1, "iso.0", "2012-03-14 08:52:48", 20 },
   { "cmp_sky_time", 1, "hms", "12:34:56", 3 },
   { "cmp_sky_time", 3, "iso.0", "2012-03-14 08:52:48", 20 },
   { "cmp_perm", 1, "iso.0", "2012-03-14 08:52:48", 20 },
   { "cmp_perm", 2, "hms", "12:34:56", 3 },
   { "specflux", 2, "%g", "1.5", 3 },
   { "frameset_sky", 2, "dms", "-01:02:03", 3 },
   { "frameset_sky", 1, "hms", "12:34:56", 3 },
   /* A Region's Fields is frame.c's, which reaches the TimeFrame's plain
      Axis rather than TimeFrame Fields. */
   { "box_time_iso", 1, "iso.0", "2012-03-14 08:52:48", 20 },
   { "box_time_iso", 1, "%g", "56000.5", 20 },
   { "box_sky", 1, "hms", "12:34:56", 3 },
};

#define NUMOF(a) ( (int)( sizeof( a ) / sizeof( ( a )[ 0 ] ) ) )

int main( void ) {
   int status = 0, i, j, k;
   astWatch( &status );

   printf( "// Generated by tools/oracle_fixtures/plot_frame_methods_probe.c.\n"
           "// Do not edit.\n\n"
           "// Each value is printed with 17 digits so that it reads back exactly.\n"
           "#![allow(clippy::excessive_precision, clippy::approx_constant)]\n\n"
           "use starlink_ast::constants::BAD_VALUE as BAD;\n\n"
           "/// `(frame id, axis, gap, result, ntick)` from astGap; `ntick` is\n"
           "/// `None` when C left it unchanged.\n"
           "pub type GapCase = (&'static str, usize, f64, f64, Option<i32>);\n\n"
           "/// `(frame id, axis, value, gap, result)` from astCentre.\n"
           "pub type CentreCase = (&'static str, usize, f64, f64, f64);\n\n"
           "/// `(frame id, axis, fmt, str1, str2, offset into str2)` from astAbbrev.\n"
           "pub type AbbrevCase = (&'static str, usize, &'static str, &'static str, &'static str, usize);\n\n"
           "/// `(frame id, axis, fmt, text, maxfld, (start, len) per field, value)`\n"
           "/// from astFields.\n"
           "pub type FieldsCase = (&'static str, usize, &'static str, &'static str, usize, &'static [(usize, usize)], f64);\n\n" );

   printf( "pub const GAP_CASES: &[GapCase] = &[\n" );
   for( i = 0; i < NUMOF( axes ); i++ ) {
      AstFrame *f = make_frame( axes[ i ].id );
      void *fp = astMakePointer( f );
      for( j = 0; j < NUMOF( gaps ); j++ ) {
         int ntick = -999;
         double r = astGap_( fp, axes[ i ].axis - 1, gaps[ j ], &ntick, &status );
         printf( "    (" ); rstr( axes[ i ].id ); printf( ", %d, ", axes[ i ].axis );
         flit( gaps[ j ] ); printf( ", " ); flit( r ); printf( ", " );
         if( ntick == -999 ) printf( "None" ); else printf( "Some(%d)", ntick );
         printf( "),\n" );
      }
      f = astAnnul( f );
   }
   for( i = 0; i < NUMOF( time_axes ); i++ ) {
      AstFrame *f = make_frame( time_axes[ i ].id );
      void *fp = astMakePointer( f );
      for( j = 0; j < NUMOF( time_gaps ); j++ ) {
         int ntick = -999;
         double r;
         /* One hour is the ladder's 1/24-day threshold, so which step it
            takes depends on the clock. */
         if( !strcmp( time_axes[ i ].id, "time_unit_h_iso" ) &&
             time_gaps[ j ] == 1.0 ) continue;
         r = astGap_( fp, 0, time_gaps[ j ], &ntick, &status );
         printf( "    (" ); rstr( time_axes[ i ].id ); printf( ", 1, " );
         flit( time_gaps[ j ] ); printf( ", " ); flit( r ); printf( ", " );
         if( ntick == -999 ) printf( "None" ); else printf( "Some(%d)", ntick );
         printf( "),\n" );
      }
      f = astAnnul( f );
   }
   for( i = 0; i < NUMOF( compound_time_axes ); i++ ) {
      AstFrame *f = make_frame( compound_time_axes[ i ].id );
      void *fp = astMakePointer( f );
      for( j = 0; j < NUMOF( time_gaps ); j++ ) {
         int ntick = -999;
         double r = astGap_( fp, compound_time_axes[ i ].axis - 1, time_gaps[ j ],
                             &ntick, &status );
         printf( "    (" ); rstr( compound_time_axes[ i ].id );
         printf( ", %d, ", compound_time_axes[ i ].axis );
         flit( time_gaps[ j ] ); printf( ", " ); flit( r ); printf( ", " );
         if( ntick == -999 ) printf( "None" ); else printf( "Some(%d)", ntick );
         printf( "),\n" );
      }
      f = astAnnul( f );
   }
   printf( "];\n\npub const CENTRE_CASES: &[CentreCase] = &[\n" );
   for( i = 0; i < NUMOF( axes ); i++ ) {
      AstFrame *f = make_frame( axes[ i ].id );
      void *fp = astMakePointer( f );
      for( j = 0; j < NUMOF( centres ); j++ ) {
         double r = astCentre_( fp, axes[ i ].axis - 1, centres[ j ][ 0 ],
                                centres[ j ][ 1 ], &status );
         printf( "    (" ); rstr( axes[ i ].id ); printf( ", %d, ", axes[ i ].axis );
         flit( centres[ j ][ 0 ] ); printf( ", " ); flit( centres[ j ][ 1 ] );
         printf( ", " ); flit( r ); printf( "),\n" );
      }
      f = astAnnul( f );
   }
   for( i = 0; i < NUMOF( time_centres ); i++ ) {
      const struct time_centre_case *c = time_centres + i;
      AstFrame *f = make_frame( c->id );
      double r = astCentre_( astMakePointer( f ), 0, c->value, c->gap, &status );
      printf( "    (" ); rstr( c->id ); printf( ", 1, " );
      flit( c->value ); printf( ", " ); flit( c->gap ); printf( ", " );
      flit( r ); printf( "),\n" );
      f = astAnnul( f );
   }
   printf( "];\n\npub const ABBREV_CASES: &[AbbrevCase] = &[\n" );
   for( i = 0; i < NUMOF( abbrevs ); i++ ) {
      const struct abbrev_case *c = abbrevs + i;
      AstFrame *f = make_frame( c->id );
      void *fp = astMakePointer( f );
      const char *r = astAbbrev_( fp, c->axis - 1, c->fmt, c->str1, c->str2,
                                  &status );
      printf( "    (" ); rstr( c->id ); printf( ", %d, ", c->axis );
      rstr( c->fmt ); printf( ", " ); rstr( c->str1 ); printf( ", " );
      rstr( c->str2 ); printf( ", %d),\n", (int)( r - c->str2 ) );
      f = astAnnul( f );
   }
   printf( "];\n\npub const FIELDS_CASES: &[FieldsCase] = &[\n" );
   for( i = 0; i < NUMOF( fieldss ); i++ ) {
      const struct fields_case *c = fieldss + i;
      AstFrame *f = make_frame( c->id );
      void *fp = astMakePointer( f );
      char *fields[ MAXFLD ];
      int nc[ MAXFLD ];
      double val = -12345.0;
      int nf = astFields_( fp, c->axis - 1, c->fmt, c->text, c->maxfld,
                           fields, nc, &val, &status );
      printf( "    (" ); rstr( c->id ); printf( ", %d, ", c->axis );
      rstr( c->fmt ); printf( ", " ); rstr( c->text );
      printf( ", %d, &[", c->maxfld );
      for( k = 0; k < nf; k++ ) {
         printf( "%s(%d, %d)", k ? ", " : "", (int)( fields[ k ] - c->text ),
                 nc[ k ] );
      }
      printf( "], " ); flit( val ); printf( "),\n" );
      f = astAnnul( f );
   }
   printf( "];\n" );

   if( !astOK ) {
      fprintf( stderr, "AST error status %d\n", status );
      return 1;
   }
   return 0;
}
