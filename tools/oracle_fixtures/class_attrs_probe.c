/* class_attrs_probe -- capture the class-specific attributes of TimeFrame,
 * SpecFrame, DSBSpecFrame and FluxFrame as the C library reports them: each
 * class's defaults, the value and astTest result of every attribute after
 * each setting, and the Native dump for each setting. The Frame attributes
 * whose defaults these classes supply (Title, Domain, Label, Symbol, Unit)
 * are read too, since they follow System and the class attributes.
 *
 * C reads per-wavelength surface brightness as "SRCBR" but writes it as
 * "SFCBRW" (fluxframe.c:733, :3203), so the setting uses "SRCBR".
 *
 * Settings are applied with astSet in C's attribute syntax, so they carry
 * C's units: RestFreq, DSBCentre and IF in GHz, SourceVel in km/s, RefRA in
 * hours, RefDec and the TimeFrame's LTOffset in degrees and hours, and
 * SpecOrigin, SpecVal and TimeOrigin in the Frame's current units. Several
 * settings may be given, separated by ";".
 *
 * Prints Rust source for crates/starlink-ast/tests/class_attrs_oracle/cases.rs:
 *
 *   ./class_attrs_probe > ../../crates/starlink-ast/tests/class_attrs_oracle/cases.rs
 */
#include <float.h>
#include <stdio.h>
#include <string.h>
#include "ast.h"

typedef struct { const char *name; char kind; int ro; } Attr;

static const char *time_settings[] = {
   "", "System=JD", "System=JEPOCH", "System=BEPOCH", "TimeScale=TAI",
   "TimeScale=LMST", "TimeScale=LT", "TimeScale=LT;LTOffset=5.5",
   "AlignTimeScale=TT", "AlignSystem=JD", "TimeOrigin=58000",
   "System=JD;TimeOrigin=2458000", "Format(1)=iso.2;TimeOrigin=58000",
   "Unit(1)=s;TimeOrigin=86400", "System=BEPOCH;TimeOrigin=2000", NULL
};
static const Attr time_attrs[] = {
   { "System", 's', 0 }, { "AlignSystem", 's', 0 }, { "TimeScale", 's', 0 },
   { "AlignTimeScale", 's', 0 }, { "TimeOrigin", 'd', 0 },
   { "LTOffset", 'd', 0 }, { "Title", 's', 0 }, { "Domain", 's', 0 },
   { "Label(1)", 's', 0 }, { "Symbol(1)", 's', 0 }, { "Unit(1)", 's', 0 },
   { NULL, 0, 0 }
};

static const char *spec_settings[] = {
   "", "System=FREQ", "System=ENER", "System=WAVN", "System=AWAV",
   "System=VRAD", "System=VOPT", "System=ZOPT", "System=BETA", "System=VREL",
   "StdOfRest=LSRK", "StdOfRest=Topocentric", "AlignStdOfRest=Barycentric",
   "AlignSystem=FREQ", "RestFreq=230", "RefRA=1:00:00", "RefDec=-30:00:00",
   "SourceVel=10", "SourceVRF=LSRD", "SourceSys=ZOPT", "SourceSys=ZOPT;SourceVel=0.1",
   "SpecOrigin=5000", "System=FREQ;SpecOrigin=1E14", "AlignSpecOffset=1",
   "System=VRAD;RestFreq=230", "System=VRAD;RestFreq=230;SpecOrigin=10", NULL
};
static const Attr spec_attrs[] = {
   { "System", 's', 0 }, { "AlignSystem", 's', 0 }, { "StdOfRest", 's', 0 },
   { "AlignStdOfRest", 's', 0 }, { "RestFreq", 'd', 0 }, { "RefRA", 's', 0 },
   { "RefDec", 's', 0 }, { "SourceVel", 'd', 0 }, { "SourceVRF", 's', 0 },
   { "SourceSys", 's', 0 }, { "SpecOrigin", 'd', 0 },
   { "AlignSpecOffset", 'i', 0 }, { "Title", 's', 0 }, { "Domain", 's', 0 },
   { "Label(1)", 's', 0 }, { "Symbol(1)", 's', 0 }, { "Unit(1)", 's', 0 },
   { NULL, 0, 0 }
};

static const char *dsb_settings[] = {
   "", "DSBCentre=230", "IF=4", "SideBand=USB", "SideBand=LO",
   "AlignSideBand=1", "DSBCentre=230;IF=4", "DSBCentre=230;IF=4;SideBand=USB",
   "System=FREQ;DSBCentre=230;IF=4", "System=FREQ;DSBCentre=230;IF=-4",
   "System=VRAD;RestFreq=230;DSBCentre=230;IF=4", NULL
};
static const Attr dsb_attrs[] = {
   { "System", 's', 0 }, { "DSBCentre", 'd', 0 }, { "IF", 'd', 0 },
   { "SideBand", 's', 0 }, { "AlignSideBand", 'i', 0 }, { "ImagFreq", 'd', 1 },
   { "RestFreq", 'd', 0 }, { "Title", 's', 0 }, { "Domain", 's', 0 },
   { "Label(1)", 's', 0 }, { "Symbol(1)", 's', 0 }, { "Unit(1)", 's', 0 },
   { NULL, 0, 0 }
};

static const char *flux_settings[] = {
   "", "System=FLXDNW", "System=SFCBR", "System=SRCBR", "AlignSystem=FLXDNW",
   "SpecVal=1E10", "Unit(1)=Jy", "Unit(1)=W/m^2/Angstrom",
   "System=SFCBR;Unit(1)=Jy/sr", NULL
};
static const Attr flux_attrs[] = {
   { "System", 's', 0 }, { "AlignSystem", 's', 0 }, { "SpecVal", 's', 0 },
   { "Title", 's', 0 }, { "Domain", 's', 0 }, { "Label(1)", 's', 0 },
   { "Symbol(1)", 's', 0 }, { "Unit(1)", 's', 0 }, { NULL, 0, 0 }
};

static const struct {
   const char *cls;
   const char **settings;
   const Attr *attrs;
} classes[] = {
   { "TimeFrame", time_settings, time_attrs },
   { "SpecFrame", spec_settings, spec_attrs },
   { "DSBSpecFrame", dsb_settings, dsb_attrs },
   { "FluxFrame", flux_settings, flux_attrs },
   { NULL, NULL, NULL }
};

static void lit( const char *s ) {
   putchar( '"' );
   for( ; *s; s++ ) {
      if( *s == '"' || *s == '\\' ) putchar( '\\' );
      putchar( *s );
   }
   putchar( '"' );
}

static void flit( double value ) {
   char buf[ 40 ];
   if( value == AST__BAD ) { fputs( "BAD", stdout ); return; }
   if( value == DBL_MAX ) { fputs( "f64::MAX", stdout ); return; }
   snprintf( buf, sizeof( buf ), "%.17g", value );
   fputs( buf, stdout );
   if( !strpbrk( buf, ".en" ) ) fputs( ".0", stdout );
}

static AstFrame *make( const char *cls, const char *setting ) {
   AstFrame *f;
   char buf[ 200 ], *tok, *save;
   if( !strcmp( cls, "TimeFrame" ) ) f = (AstFrame *) astTimeFrame( " " );
   else if( !strcmp( cls, "SpecFrame" ) ) f = (AstFrame *) astSpecFrame( " " );
   else if( !strcmp( cls, "DSBSpecFrame" ) ) f = (AstFrame *) astDSBSpecFrame( " " );
   else f = (AstFrame *) astFluxFrame( AST__BAD, NULL, " " );
   snprintf( buf, sizeof( buf ), "%s", setting );
   for( tok = strtok_r( buf, ";", &save ); tok; tok = strtok_r( NULL, ";", &save ) ) {
      astSet( f, "%s", tok );
      if( !astOK ) astClearStatus;
   }
   return f;
}

static char dumpbuf[ 65536 ];
static void sink( const char *line ) {
   strncat( dumpbuf, line, sizeof( dumpbuf ) - strlen( dumpbuf ) - 2 );
   strcat( dumpbuf, "\n" );
}

int main( void ) {
   int status = 0;
   int c, s, a;
   astWatch( &status );

   printf( "// Generated by tools/oracle_fixtures/class_attrs_probe.c against\n"
           "// the C library at the submodule pin. Do not edit.\n\n"
           "// Each value is printed with 17 digits so that it reads back exactly.\n"
           "#![allow(clippy::excessive_precision)]\n\n"
           "#[allow(unused_imports)]\n"
           "use starlink_ast::constants::BAD_VALUE as BAD;\n\n"
           "/// An attribute value as C reports it.\n"
           "#[derive(Debug, Clone, Copy, PartialEq)]\n"
           "pub enum V {\n    S(&'static str),\n    I(i32),\n    D(f64),\n}\n\n"
           "/// `(class, settings, attribute, value, astTest result)`; the test\n"
           "/// result is `None` for a read-only attribute.\n"
           "pub type ValueCase = (&'static str, &'static str, &'static str, V, Option<bool>);\n\n"
           "pub const VALUE_CASES: &[ValueCase] = &[\n" );

   for( c = 0; classes[ c ].cls; c++ ) {
      for( s = 0; classes[ c ].settings[ s ]; s++ ) {
         AstFrame *f = make( classes[ c ].cls, classes[ c ].settings[ s ] );
         for( a = 0; classes[ c ].attrs[ a ].name; a++ ) {
            const Attr *at = &classes[ c ].attrs[ a ];
            printf( "    (" ); lit( classes[ c ].cls ); printf( ", " );
            lit( classes[ c ].settings[ s ] ); printf( ", " ); lit( at->name );
            printf( ", " );
            if( at->kind == 's' ) {
               const char *v = astGetC( f, at->name );
               printf( "V::S(" ); lit( v ? v : "<NULL>" ); printf( ")" );
            } else if( at->kind == 'i' ) {
               printf( "V::I(%d)", astGetI( f, at->name ) );
            } else {
               printf( "V::D(" ); flit( astGetD( f, at->name ) ); printf( ")" );
            }
            if( at->ro ) {
               printf( ", None),\n" );
            } else {
               printf( ", Some(%s)),\n", astTest( f, at->name ) ? "true" : "false" );
            }
            if( !astOK ) astClearStatus;
         }
         f = astAnnul( f );
      }
   }
   printf( "];\n\n" );

   printf( "/// `(class, settings, astWrite dump)`.\n"
           "pub const DUMP_CASES: &[(&str, &str, &str)] = &[\n" );
   for( c = 0; classes[ c ].cls; c++ ) {
      for( s = 0; classes[ c ].settings[ s ]; s++ ) {
         AstFrame *f = make( classes[ c ].cls, classes[ c ].settings[ s ] );
         AstChannel *ch = astChannel( NULL, sink, " " );
         dumpbuf[ 0 ] = '\0';
         astWrite( ch, f );
         ch = astAnnul( ch );
         f = astAnnul( f );
         printf( "    (\n        " ); lit( classes[ c ].cls ); printf( ", " );
         lit( classes[ c ].settings[ s ] );
         printf( ",\n        r#\"%s\"#,\n    ),\n", dumpbuf );
      }
   }
   printf( "];\n" );
   return astOK ? 0 : 1;
}
