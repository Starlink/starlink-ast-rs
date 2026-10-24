/* fitschan_tab_write_probe -- capture how a FitsChan (fitschan.c) writes
 * FrameSets whose axes need the "-TAB" algorithm of FITS-WCS paper III: the
 * header cards, the look-up tables left in the FitsChan, and the FrameSet
 * read back from the header and tables.
 *
 * Each case builds a FrameSet and writes it to a new FitsChan holding the
 * given cards and with the given attributes set. The transcript is:
 *
 *   = written <n>        the value astWrite returns (or "! <status>")
 *   |<card>              each card left in the FitsChan
 *   = tables <keys>      astGetTables: "null", or the keys in key order
 *   |<line>              the dump of each table, then "~" and the cards of
 *                        its astGetTableHeader
 *   = read <class>       astRead of the header, with TabOK=1, the tables
 *                        still in the FitsChan (or "! <status>")
 *   |<line>              the dump of what was read
 *
 * The FrameSet written is printed as a native dump, which the Rust test
 * reads to write the same FrameSet.
 *
 * Prints Rust source for crates/starlink-ast/tests/fitschan_tab_write_oracle/cases.rs:
 *
 *   ./fitschan_tab_write_probe > ../../crates/starlink-ast/tests/fitschan_tab_write_oracle/cases.rs
 */
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"

static char lines[ 20000 ][ 200 ];
static int nline;
static char out[ 4000000 ];
static char fsdump[ 400000 ];

static void sink( const char *line ) {
   strncpy( lines[ nline ], line, 199 );
   lines[ nline++ ][ 199 ] = 0;
}

static void add( const char *fmt, const char *s ) {
   char buf[ 2000 ];
   snprintf( buf, sizeof( buf ), fmt, s );
   strcat( out, buf );
}

static void addi( const char *fmt, long long v ) {
   char buf[ 100 ];
   snprintf( buf, sizeof( buf ), fmt, v );
   strcat( out, buf );
}

/* Dump "obj" to "buf", each line prefixed by "prefix" and ending "\n". */
static void dump_to( AstObject *obj, char *buf, const char *prefix ) {
   AstChannel *ch;
   int i;
   nline = 0;
   ch = astChannel( NULL, sink, " " );
   astWrite( ch, obj );
   ch = astAnnul( ch );
   for( i = 0; i < nline; i++ ) {
      strcat( buf, prefix );
      strcat( buf, lines[ i ] );
      strcat( buf, "\n" );
   }
}

static void cards( AstFitsChan *fc ) {
   char card[ 81 ];
   astClear( fc, "Card" );
   while( astFindFits( fc, "%f", card, 1 ) ) add( "|%s\n", card );
}

static void str( const char *s ) {
   putchar( '"' );
   for( ; *s; s++ ) {
      if( *s == '"' || *s == '\\' ) putchar( '\\' );
      if( *s == '\n' ) {
         printf( "\\n" );
      } else {
         putchar( *s );
      }
   }
   putchar( '"' );
}

/* Write "fs" to a FitsChan holding the ';'-separated cards "hdr" and with
   attributes "attrs", and record the transcript. */
static void emit( const char *name, AstFrameSet *fs, const char *attrs,
                  const char *hdr ) {
   AstFitsChan *fc;
   AstKeyMap *km;
   AstObject *obj;
   char buf[ 2000 ], *p, *q;
   int i, n;

   out[ 0 ] = 0;
   fsdump[ 0 ] = 0;
   dump_to( (AstObject *) fs, fsdump, "" );

   fc = astFitsChan( NULL, NULL, "%s", attrs );
   strcpy( buf, hdr );
   for( p = buf; *p; p = q ) {
      q = strchr( p, ';' );
      if( q ) {
         *q++ = 0;
      } else {
         q = p + strlen( p );
      }
      astPutFits( fc, p, 0 );
   }

   n = astWrite( fc, fs );
   if( astOK ) {
      addi( "= written %lld\n", n );
   } else {
      addi( "! %lld\n", astStatus );
      astClearStatus;
   }
   cards( fc );

   km = astGetTables( fc );
   if( !km ) {
      add( "= tables null%s\n", "" );
   } else {
      astSet( km, "SortBy=KeyUp" );
      add( "= tables%s", "" );
      for( i = 0; i < astMapSize( km ); i++ ) add( " %s", astMapKey( km, i ) );
      add( "%s\n", "" );
      for( i = 0; i < astMapSize( km ); i++ ) {
         if( astMapGet0A( km, astMapKey( km, i ), &obj ) ) {
            AstFitsChan *th;
            dump_to( obj, out, "|" );
            add( "~%s\n", "" );
            th = astGetTableHeader( (AstFitsTable *) obj );
            cards( th );
            th = astAnnul( th );
            obj = astAnnul( obj );
         }
      }
      km = astAnnul( km );
   }

   astSetI( fc, "TabOK", 1 );
   astClear( fc, "Card" );
   obj = astRead( fc );
   if( !astOK ) {
      addi( "! %lld\n", astStatus );
      astClearStatus;
   } else if( obj ) {
      add( "= read %s\n", astGetC( obj, "Class" ) );
      dump_to( obj, out, "|" );
      obj = astAnnul( obj );
   } else {
      add( "= read null%s\n", "" );
   }
   fc = astAnnul( fc );

   printf( "    (\"%s\",\n     ", name );
   str( attrs );
   printf( ",\n     " );
   str( hdr );
   printf( ",\n     " );
   str( fsdump );
   printf( ",\n     " );
   str( out );
   printf( "),\n" );
}

/* A FrameSet from a "nin"-axis GRID Frame through "map" to "frm". */
static AstFrameSet *fset( int nin, AstMapping *map, AstFrame *frm ) {
   AstFrameSet *fs = astFrameSet( astFrame( nin, "Domain=GRID" ), " " );
   astAddFrame( fs, AST__BASE, map, frm );
   return fs;
}

static AstMapping *recip( void ) {
   const char *fwd[] = { "y=1/(x*x)" };
   const char *inv[] = { "x=1/sqrt(y)" };
   return (AstMapping *) astMathMap( 1, 1, 1, fwd, 1, inv, " " );
}

int main( void ) {
   int status = 0, i;
   double lut5[ 5 ] = { 0.0, 0.5, 1.5, 3.0, 5.0 };
   double lut100[ 100 ];
   double ralut[ 4 ] = { 0.1, 0.11, 0.13, 0.16 };
   double declut[ 3 ] = { -0.5, -0.49, -0.47 };
   double shift;
   AstFrameSet *fs;
   AstObject *obj;
   AstChannel *ch;

   astWatch( &status );

   printf( "// Generated by tools/oracle_fixtures/fitschan_tab_write_probe.c. Do not edit.\n\n" );
   printf( "/// `(name, attributes, cards, FrameSet dump, transcript)`: see the probe\n"
           "/// for the transcript format.\n" );
   printf( "pub type Case = (&'static str, &'static str, &'static str, &'static str, &'static str);\n\n" );
   printf( "pub const CASES: &[Case] = &[\n" );

   /* testfitschan.c testtabspectrum. */
   fs = fset( 1, (AstMapping *) astLutMap( 5, lut5, 1.0, 1.0, " " ),
              (AstFrame *) astSpecFrame( "System=wave,Unit=nm" ) );
   emit( "lut_spectrum", fs, "Encoding=FITS-WCS,CDMatrix=1,TabOK=1", "" );
   emit( "lut_spectrum_tabok_zero", fs, "Encoding=FITS-WCS,CDMatrix=1", "" );
   emit( "lut_spectrum_extver", fs, "Encoding=FITS-WCS,TabOK=3", "" );
   emit( "lut_spectrum_pc", fs, "Encoding=FITS-PC,TabOK=1", "" );
   fs = astAnnul( fs );

   /* testfitschan.c checktab: a sampled MathMap. */
   fs = fset( 1, recip(), (AstFrame *) astSpecFrame( "system=freq,unit=MHz" ) );
   emit( "sampled_spectrum", fs, "Encoding=FITS-WCS,TabOK=1", "NAXIS   = 1;NAXIS1  = 100" );
   emit( "sampled_spectrum_no_dim", fs, "Encoding=FITS-WCS,TabOK=1", "" );
   fs = astAnnul( fs );

   /* checktab: a LutMap, re-expressed as frequency, after a shift. */
   for( i = 0; i < 100; i++ ) lut100[ i ] = 1.0 / (double)( ( i + 1 )*( i + 1 ) );
   fs = fset( 1, (AstMapping *) astLutMap( 100, lut100, -49.0, 1.0, " " ),
              (AstFrame *) astSpecFrame( "System=Wave,Unit=m" ) );
   astSet( fs, "System=freq" );
   shift = 50.0;
   astRemapFrame( fs, AST__BASE, astShiftMap( 1, &shift, " " ) );
   emit( "lut_frequency", fs, "Encoding=FITS-WCS,TabOK=1", "NAXIS   = 1;NAXIS1  = 100" );
   fs = astAnnul( fs );

   /* checktab: a non-spectral axis. */
   fs = fset( 1, recip(), astFrame( 1, "domain=voltage,unit=V" ) );
   emit( "voltage", fs, "Encoding=FITS-WCS,TabOK=1", "NAXIS   = 1;NAXIS1  = 100" );
   fs = astAnnul( fs );

   /* A LutMap after a ZoomMap, which needs an index vector. */
   fs = fset( 1, (AstMapping *) astCmpMap( astZoomMap( 1, 2.0, " " ),
                                          astLutMap( 5, lut5, 1.0, 1.0, " " ), 1, " " ),
              (AstFrame *) astSpecFrame( "System=wave,Unit=nm" ) );
   emit( "index_vector", fs, "Encoding=FITS-WCS,TabOK=1", "NAXIS   = 1;NAXIS1  = 10" );
   fs = astAnnul( fs );

   /* A linear axis, written as -TAB only with ForceTab set. */
   fs = fset( 1, (AstMapping *) astZoomMap( 1, 2.0, " " ),
              (AstFrame *) astSpecFrame( "System=freq,Unit=GHz" ) );
   emit( "linear_force_tab", fs, "Encoding=FITS-WCS,TabOK=1,ForceTab=1", "NAXIS   = 1;NAXIS1  = 10" );
   emit( "linear_no_force", fs, "Encoding=FITS-WCS,TabOK=1", "NAXIS   = 1;NAXIS1  = 10" );
   fs = astAnnul( fs );

   /* Celestial axes from look-up tables, without and with a SkyRef. */
   fs = fset( 2, (AstMapping *) astCmpMap( astLutMap( 4, ralut, 1.0, 1.0, " " ),
                                          astLutMap( 3, declut, 1.0, 1.0, " " ), 0, " " ),
              (AstFrame *) astSkyFrame( " " ) );
   emit( "celestial", fs, "Encoding=FITS-WCS,TabOK=1", "NAXIS   = 2;NAXIS1  = 4;NAXIS2  = 3" );
   astSetD( fs, "SkyRef(1)", 0.115 );
   astSetD( fs, "SkyRef(2)", -0.485 );
   astSet( fs, "SkyRefIs=Ignored" );
   emit( "celestial_skyref", fs, "Encoding=FITS-WCS,TabOK=1", "NAXIS   = 2;NAXIS1  = 4;NAXIS2  = 3" );
   fs = astAnnul( fs );

   /* Celestial axes from a sampled Mapping defined only forwards, so the
      reference position goes in AXREF. */
   {
      const char *fwd[] = { "lon=0.1+0.01*x*x", "lat=-0.5+0.01*y*y" };
      const char *inv[] = { "x", "y" };
      AstMapping *mm = (AstMapping *) astMathMap( 2, 2, 2, fwd, 2, inv, " " );
      fs = fset( 2, mm, (AstFrame *) astSkyFrame( "SkyRefIs=Ignored" ) );
      astSetD( fs, "SkyRef(1)", 0.12 );
      astSetD( fs, "SkyRef(2)", -0.48 );
   }
   emit( "celestial_forward_only", fs, "Encoding=FITS-WCS,TabOK=1", "NAXIS   = 2;NAXIS1  = 10;NAXIS2  = 10" );
   fs = astAnnul( fs );

   /* testfitschan.c checktab: sparse.ast. */
   ch = astChannel( NULL, NULL, "SourceFile=../../reference/starlink-ast/ast_tester/fixtures/programs/testfitschan/sparse.ast" );
   obj = astRead( ch );
   ch = astAnnul( ch );
   emit( "sparse", (AstFrameSet *) obj, "Encoding=FITS-WCS,TabOK=1",
         "NAXIS   = 2;NAXIS1  = 2000;NAXIS2  = 1" );
   obj = astAnnul( obj );

   printf( "];\n" );
   return 0;
}
