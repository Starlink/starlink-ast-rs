/* frame_axis_attrs_probe -- capture the per-axis attributes (Label, Symbol,
 * Unit, Format, Digits, Direction, Top, Bottom) of every Frame class as the
 * C library reports them: each class's defaults, and the value and astTest
 * result on every axis after one attribute is set on the first or the last
 * axis, including what each component of a compound Frame, a FrameSet's
 * current Frame and a Region's Frame then report.
 *
 * Each attribute is read, set and tested through the C function the Frame
 * class declares (astGetLabel and so on), which is what the Rust `Frame`
 * trait mirrors: for a CmpFrame these differ from astGetC and astSet, which
 * hand an axis-qualified attribute straight to the primary Frame
 * (cmpframe.c GetAttrib) and so skip the CmpFrame's own over-rides.
 * Digits(axis) has no such function, so it goes through the attribute
 * string, as it does in C.
 *
 * Prints Rust source for crates/starlink-ast/tests/frame_axis_oracle/cases.rs:
 *
 *   ./frame_axis_attrs_probe > ../../crates/starlink-ast/tests/frame_axis_oracle/cases.rs
 */
#include <float.h>
#include <stdio.h>
#include <string.h>
#include "ast.h"

/* The protected per-axis accessors (frame.h); `axis` is zero-based. */
extern const char *astGetLabel_( void *, int, int * );
extern const char *astGetSymbol_( void *, int, int * );
extern const char *astGetUnit_( void *, int, int * );
extern const char *astGetFormat_( void *, int, int * );
extern int astGetDirection_( void *, int, int * );
extern double astGetTop_( void *, int, int * );
extern double astGetBottom_( void *, int, int * );
extern int astTestLabel_( void *, int, int * );
extern int astTestSymbol_( void *, int, int * );
extern int astTestUnit_( void *, int, int * );
extern int astTestFormat_( void *, int, int * );
extern int astTestDirection_( void *, int, int * );
extern int astTestTop_( void *, int, int * );
extern int astTestBottom_( void *, int, int * );
extern void astSetLabel_( void *, int, const char *, int * );
extern void astSetSymbol_( void *, int, const char *, int * );
extern void astSetUnit_( void *, int, const char *, int * );
extern void astSetFormat_( void *, int, const char *, int * );
extern void astSetDirection_( void *, int, int, int * );
extern void astSetTop_( void *, int, double, int * );
extern void astSetBottom_( void *, int, double, int * );

static const char *classes[] = {
   "Frame", "SkyFrame", "SpecFrame", "DSBSpecFrame", "TimeFrame",
   "FluxFrame", "SpecFluxFrame", "CmpFrame", "PlainCmpFrame", "FrameSet", "Circle", NULL
};

static const struct { const char *name; } attrs[] = {
   { "Label" }, { "Symbol" }, { "Unit" }, { "Format" }, { "Digits" },
   { "Direction" }, { "Top" }, { "Bottom" }, { NULL }
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
   if( value == DBL_MAX ) { fputs( "f64::MAX", stdout ); return; }
   if( value == -DBL_MAX ) { fputs( "-f64::MAX", stdout ); return; }
   snprintf( buf, sizeof( buf ), "%.17g", value );
   fputs( buf, stdout );
   if( !strpbrk( buf, ".en" ) ) fputs( ".0", stdout );
}

/* Build one object of a class; the Rust test builds the same. */
static AstFrame *make( const char *cls ) {
   if( !strcmp( cls, "Frame" ) ) return astFrame( 2, " " );
   if( !strcmp( cls, "SkyFrame" ) ) return (AstFrame *) astSkyFrame( " " );
   if( !strcmp( cls, "SpecFrame" ) ) return (AstFrame *) astSpecFrame( " " );
   if( !strcmp( cls, "DSBSpecFrame" ) ) return (AstFrame *) astDSBSpecFrame( " " );
   if( !strcmp( cls, "TimeFrame" ) ) return (AstFrame *) astTimeFrame( " " );
   if( !strcmp( cls, "FluxFrame" ) ) return (AstFrame *) astFluxFrame( AST__BAD, NULL, " " );
   if( !strcmp( cls, "SpecFluxFrame" ) ) {
      return (AstFrame *) astSpecFluxFrame( astSpecFrame( " " ),
                                            astFluxFrame( AST__BAD, NULL, " " ), " " );
   }
   if( !strcmp( cls, "CmpFrame" ) ) {
      return (AstFrame *) astCmpFrame( astSkyFrame( " " ), astSpecFrame( " " ), " " );
   }
   /* Plain Frame components take their default Label and Symbol from the
      CmpFrame's axis numbering. */
   if( !strcmp( cls, "PlainCmpFrame" ) ) {
      return (AstFrame *) astCmpFrame( astFrame( 1, " " ), astFrame( 2, " " ), " " );
   }
   if( !strcmp( cls, "FrameSet" ) ) return (AstFrame *) astFrameSet( astSkyFrame( " " ), " " );
   if( !strcmp( cls, "Circle" ) ) {
      double c[ 2 ] = { 0.0, 0.0 }, r = 0.01;
      return (AstFrame *) astCircle( astSkyFrame( " " ), 1, c, &r, NULL, " " );
   }
   return NULL;
}

/* The value each attribute is set to on an axis of a given Domain. The
   Unit must be one the axis accepts. */
static const char *value_for( const char *attr, const char *domain ) {
   if( !strcmp( attr, "Label" ) ) return "L x";
   if( !strcmp( attr, "Symbol" ) ) return "S";
   if( !strcmp( attr, "Unit" ) ) {
      if( !strcmp( domain, "SKY" ) ) return "rad";
      if( !strcmp( domain, "SPECTRUM" ) || !strcmp( domain, "DSBSPECTRUM" ) ) return "nm";
      if( !strcmp( domain, "TIME" ) ) return "h";
      if( !strcmp( domain, "FLUX" ) ) return "W/m^2/Hz";
      return "m";
   }
   if( !strcmp( attr, "Format" ) ) {
      if( !strcmp( domain, "SKY" ) ) return "dms.1";
      if( !strcmp( domain, "TIME" ) ) return "iso.1";
      return "%.3f";
   }
   if( !strcmp( attr, "Digits" ) ) return "3";
   if( !strcmp( attr, "Direction" ) ) return "0";
   if( !strcmp( attr, "Top" ) ) return "5";
   return "-5";
}

/* The Domain of the Frame holding an axis, which picks the value set. */
static const char *axis_domain( AstFrame *f, int axis ) {
   static char buf[ 40 ];
   int axes[ 1 ];
   AstFrame *pick;
   axes[ 0 ] = axis;
   pick = astPickAxes( f, 1, axes, NULL );
   snprintf( buf, sizeof( buf ), "%s", astGetC( pick, "Domain" ) );
   pick = astAnnul( pick );
   return buf;
}

/* Read one attribute of one (one-based) axis. */
static void get( AstFrame *f, const char *attr, int axis ) {
   void *p = astMakePointer( f );
   int st = 0, a = axis - 1;
   const char *v = NULL;
   char name[ 40 ];
   if( !strcmp( attr, "Label" ) ) v = astGetLabel_( p, a, &st );
   else if( !strcmp( attr, "Symbol" ) ) v = astGetSymbol_( p, a, &st );
   else if( !strcmp( attr, "Unit" ) ) v = astGetUnit_( p, a, &st );
   else if( !strcmp( attr, "Format" ) ) v = astGetFormat_( p, a, &st );
   else if( !strcmp( attr, "Direction" ) ) {
      printf( "V::I(%d)", astGetDirection_( p, a, &st ) );
      return;
   } else if( !strcmp( attr, "Top" ) ) {
      printf( "V::D(" ); flit( astGetTop_( p, a, &st ) ); printf( ")" );
      return;
   } else if( !strcmp( attr, "Bottom" ) ) {
      printf( "V::D(" ); flit( astGetBottom_( p, a, &st ) ); printf( ")" );
      return;
   } else {
      snprintf( name, sizeof( name ), "Digits(%d)", axis );
      printf( "V::I(%d)", astGetI( f, name ) );
      return;
   }
   printf( "V::S(" ); lit( v ? v : "<NULL>" ); printf( ")" );
}

static int test( AstFrame *f, const char *attr, int axis ) {
   void *p = astMakePointer( f );
   int st = 0, a = axis - 1;
   char name[ 40 ];
   if( !strcmp( attr, "Label" ) ) return astTestLabel_( p, a, &st );
   if( !strcmp( attr, "Symbol" ) ) return astTestSymbol_( p, a, &st );
   if( !strcmp( attr, "Unit" ) ) return astTestUnit_( p, a, &st );
   if( !strcmp( attr, "Format" ) ) return astTestFormat_( p, a, &st );
   if( !strcmp( attr, "Direction" ) ) return astTestDirection_( p, a, &st );
   if( !strcmp( attr, "Top" ) ) return astTestTop_( p, a, &st );
   if( !strcmp( attr, "Bottom" ) ) return astTestBottom_( p, a, &st );
   snprintf( name, sizeof( name ), "Digits(%d)", axis );
   return astTest( f, name );
}

static void set( AstFrame *f, const char *attr, int axis, const char *value ) {
   void *p = astMakePointer( f );
   int st = 0, a = axis - 1;
   double d;
   int i;
   if( !strcmp( attr, "Label" ) ) astSetLabel_( p, a, value, &st );
   else if( !strcmp( attr, "Symbol" ) ) astSetSymbol_( p, a, value, &st );
   else if( !strcmp( attr, "Unit" ) ) astSetUnit_( p, a, value, &st );
   else if( !strcmp( attr, "Format" ) ) astSetFormat_( p, a, value, &st );
   else if( !strcmp( attr, "Direction" ) ) {
      sscanf( value, "%d", &i );
      astSetDirection_( p, a, i, &st );
   } else if( !strcmp( attr, "Top" ) ) {
      sscanf( value, "%lf", &d );
      astSetTop_( p, a, d, &st );
   } else if( !strcmp( attr, "Bottom" ) ) {
      sscanf( value, "%lf", &d );
      astSetBottom_( p, a, d, &st );
   } else {
      astSet( f, "Digits(%d)=%s", axis, value );
   }
}

static void report( const char *cls, const char *setting, const char *who,
                    AstFrame *f ) {
   int a, axis, naxes;
   char name[ 40 ];
   naxes = astGetI( f, "Naxes" );
   for( axis = 1; axis <= naxes; axis++ ) {
      for( a = 0; attrs[ a ].name; a++ ) {
         snprintf( name, sizeof( name ), "%s(%d)", attrs[ a ].name, axis );
         printf( "    (" ); lit( cls ); printf( ", " ); lit( setting );
         printf( ", " ); lit( who ); printf( ", " ); lit( name );
         printf( ", " );
         get( f, attrs[ a ].name, axis );
         printf( ", %s),\n", test( f, attrs[ a ].name, axis ) ? "true" : "false" );
         if( !astOK ) astClearStatus;
      }
   }
}

/* Report the object and whatever Frames show the effect of a setting. */
static void report_all( const char *cls, const char *setting, AstFrame *f ) {
   report( cls, setting, "self", f );
   if( !strcmp( cls, "CmpFrame" ) || !strcmp( cls, "PlainCmpFrame" ) ||
       !strcmp( cls, "SpecFluxFrame" ) ) {
      AstMapping *m1, *m2;
      int series, inv1, inv2;
      astDecompose( f, &m1, &m2, &series, &inv1, &inv2 );
      report( cls, setting, "inner1", (AstFrame *) m1 );
      report( cls, setting, "inner2", (AstFrame *) m2 );
      m1 = astAnnul( m1 );
      m2 = astAnnul( m2 );
   } else if( !strcmp( cls, "FrameSet" ) || !strcmp( cls, "Circle" ) ) {
      AstFrame *in = !strcmp( cls, "FrameSet" )
                     ? astGetFrame( (AstFrameSet *) f, AST__CURRENT )
                     : astGetRegionFrame( f );
      report( cls, setting, "inner1", in );
      in = astAnnul( in );
   }
}

int main( void ) {
   int status = 0;
   int c, a, naxes, ends[ 2 ], e;
   char setting[ 80 ];
   const char *value;
   AstFrame *f;
   astWatch( &status );

   printf( "// Generated by tools/oracle_fixtures/frame_axis_attrs_probe.c against\n"
           "// the C library at the submodule pin. Do not edit.\n\n"
           "// Each value is printed with 17 digits so that it reads back exactly,\n"
           "// including the limits of a latitude axis, which C computes as pi/2.\n"
           "#![allow(clippy::excessive_precision, clippy::approx_constant)]\n\n"
           "/// An attribute value as C reports it.\n"
           "#[derive(Debug, Clone, Copy, PartialEq)]\n"
           "pub enum V {\n    S(&'static str),\n    I(i32),\n    D(f64),\n}\n\n"
           "/// `(class, setting, whose, attribute, value, test)`: `whose` is\n"
           "/// \"self\" for the object itself, and \"inner1\" and \"inner2\" for its\n"
           "/// components or the Frame it presents; `setting` is empty for the\n"
           "/// defaults.\n"
           "pub type Case = (&'static str, &'static str, &'static str, &'static str, V, bool);\n\n"
           "pub const CASES: &[Case] = &[\n" );

   for( c = 0; classes[ c ]; c++ ) {
      f = make( classes[ c ] );
      report_all( classes[ c ], "", f );
      naxes = astGetI( f, "Naxes" );
      f = astAnnul( f );

      ends[ 0 ] = 1;
      ends[ 1 ] = naxes;
      for( a = 0; attrs[ a ].name; a++ ) {
         for( e = 0; e < ( naxes > 1 ? 2 : 1 ); e++ ) {
            f = make( classes[ c ] );
            value = value_for( attrs[ a ].name, axis_domain( f, ends[ e ] ) );
            snprintf( setting, sizeof( setting ), "%s(%d)=%s", attrs[ a ].name,
                      ends[ e ], value );
            set( f, attrs[ a ].name, ends[ e ], value );
            if( !astOK ) astClearStatus;
            report_all( classes[ c ], setting, f );
            f = astAnnul( f );
         }
      }

      /* The Frame's own Digits supplies the default Digits and Format of
         every axis. */
      f = make( classes[ c ] );
      astSet( f, "Digits=4" );
      report_all( classes[ c ], "Digits=4", f );
      f = astAnnul( f );
   }
   printf( "];\n" );
   return astOK ? 0 : 1;
}
