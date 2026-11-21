/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* attr_strings_probe -- apply attribute strings to objects with astSet,
 * astSetC, astGetC, astTest and astClear, and capture what the C library
 * does with each: the value or flag returned, or the error status and every
 * message reported, and the object's Native dump afterwards.
 *
 * Each case names an object the probe builds with the C constructors (or
 * reads from a FITS header), written out as Native text so that the Rust
 * test starts from the same object, and a list of operations applied to it
 * in order. A setting is applied as astSet( obj, "%s", text ), so "%" in it
 * is not a format specifier.
 *
 * The probe defines astPutErr itself to collect the messages, so it does
 * not link libast_err. The first message C reports for an error raised
 * under a public function names this file and a line ("AST: Error at line
 * ..."); it says nothing about the library and is dropped.
 *
 * Prints Rust source for crates/starlink-ast/tests/attr_strings_oracle/cases.rs.
 * Build and run from reference/starlink-ast/build (configured with
 * -DCMAKE_C_FLAGS=-ffp-contract=off), compiling grf_log.c in for the Plot
 * cases:
 *
 *   cc -ffp-contract=off -O3 -DHAVE_CONFIG_H -I. -Isrc -I../src -I.. \
 *      ../../../tools/oracle_fixtures/attr_strings_probe.c ../src/grf_log.c \
 *      -L. -last -lm -Wl,-rpath,$PWD \
 *      -o ../../../tools/oracle_fixtures/attr_strings_probe
 *   ../../../tools/oracle_fixtures/attr_strings_probe \
 *      > ../../../crates/starlink-ast/tests/attr_strings_oracle/cases.rs
 */
#include "plot_draw_probe.h"
#include "ast_err.h"

/* The messages reported since the last operation. */
#define MAXMSG 16
static char msgs[ MAXMSG ][ 1024 ];
static int nmsg = 0;

void astPutErr_( int status_value, const char *message ) {
   (void) status_value;
   if( !strncmp( message, "AST: Error", 10 ) ) return;
   if( nmsg < MAXMSG ) {
      snprintf( msgs[ nmsg ], sizeof( msgs[ nmsg ] ), "%s", message );
      nmsg++;
   }
}

enum { OP_END, OP_SET, OP_SETC, OP_GET, OP_TEST, OP_CLEAR };

struct op { int kind; const char *a; const char *b; };

#define SET( s ) { OP_SET, s, NULL }
#define SETC( n, v ) { OP_SETC, n, v }
#define GET( n ) { OP_GET, n, NULL }
#define TEST( n ) { OP_TEST, n, NULL }
#define CLEAR( n ) { OP_CLEAR, n, NULL }
/* The value and set state of an attribute. */
#define SEE( n ) GET( n ), TEST( n )
/* Set an attribute, look at it, clear it and look again. */
#define CYCLE( n, setting ) SET( setting ), SEE( n ), CLEAR( n ), SEE( n )

#define MAXOP 400
struct acase {
   const char *name;
   const char *object;
   struct op ops[ MAXOP ];
};

static const struct acase cases[] = {

/* Object attributes, as a plain Frame inherits them. */
   { "frame2_object", "frame2", {
      SEE( "ID" ), CYCLE( "ID", "ID=my id" ),
      SEE( "Ident" ), CYCLE( "Ident", "Ident=An ident" ),
      SEE( "UseDefs" ), CYCLE( "UseDefs", "UseDefs=0" ),
      SET( "UseDefs=5" ), SEE( "UseDefs" ),
      SET( "usedefs= -1 " ), GET( "UseDefs" ),
      SET( "UseDefs=x" ), SET( "UseDefs=1.5" ), SET( "UseDefs=1\t" ),
      SET( "UseDefs=" ), GET( "UseDefs" ),
      SEE( "Class" ), SET( "Class=Fred" ), CLEAR( "Class" ),
      SET( "Ident=x%^50+y%+z %% w%s70+" ), GET( "Ident" ),
      SET( "Ident=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa%+bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" ),
      GET( "Ident" ),
      SET( "Ident=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" ),
      GET( "Ident" ),
      SETC( "Ident", "a, b" ), GET( "Ident" ),
      SETC( " Ident ", "\"q\"" ), GET( "Ident" ),
      SETC( "Ident", "a,b=c" ), GET( "Ident" ),
      SETC( "Ident", "one\ntwo" ), GET( "Ident" ),
      TEST( " I dent " ), GET( " i D e n t " ),
      SET( "ID=a" ), SET( "Ident=b" ), CLEAR( " ID , Ident,UseDefs" ),
      SEE( "ID" ), SEE( "Ident" ), SEE( "UseDefs" ),
   } },

/* Mapping attributes, as a plain Frame inherits them. */
   { "frame2_mapping", "frame2", {
      SEE( "Nin" ), SEE( "Nout" ), SEE( "IsLinear" ), SEE( "IsSimple" ),
      SEE( "TranForward" ), SEE( "TranInverse" ), SEE( "Invert" ),
      SEE( "Report" ),
      SET( "Nin=3" ), SET( "Nout=" ), SET( "islinear=0" ), SET( "IsSimple=1" ),
      SET( "TranForward=1" ), SET( "traninverse=0" ),
      CLEAR( "Nin" ), CLEAR( "nout" ), CLEAR( "IsLinear" ), CLEAR( "IsSimple" ),
      CLEAR( "TranForward" ), CLEAR( "TranInverse" ),
   } },

/* Names no class has, and blank ones. */
   { "frame2_unknown", "frame2", {
      SET( "Fred=1" ), GET( "Fred" ), TEST( "Fred" ), CLEAR( "Fred" ),
      GET( " " ), TEST( " " ), CLEAR( " " ), CLEAR( " , ," ), CLEAR( "" ),
      SET( " " ), SET( "" ), SET( "Fred" ), SET( "=1" ), SET( "ID" ),
      GET( "ID=3" ), GET( "" ), TEST( "" ),
      SET( "ID=a" ), SET( "Ident=b" ), CLEAR( "ID,Fred,Ident" ),
      SEE( "ID" ), SEE( "Ident" ),
   } },

/* How astSet splits a string into assignments. */
   { "frame2_splitting", "frame2", {
      SET( "Ident=a,b" ), GET( "Ident" ),
      SET( "Ident=\"a,b\",ID=1" ), GET( "Ident" ), GET( "ID" ),
      CLEAR( "ID" ), SET( "Ident=a,b,ID=1" ), GET( "Ident" ), GET( "ID" ),
      SET( "Ident=" ), SEE( "Ident" ),
      SET( " I dent = Two words " ), GET( "Ident" ),
      SET( "Ident= \"quoted\" " ), GET( "Ident" ),
      SET( "Ident=\"quoted\"  " ), GET( "Ident" ),
      SET( "Ident=\"unclosed" ), GET( "Ident" ),
      SET( "Ident=in\"side\"" ), GET( "Ident" ),
      SET( "ID=x,UseDefs=0,Ident=y" ), GET( "ID" ), GET( "UseDefs" ),
      GET( "Ident" ),
      SET( "ID = p , ,  Ident = q " ), GET( "ID" ), GET( "Ident" ),
      SET( "ID=a=b" ), GET( "ID" ),
      SET( "ID=a=b,c" ), GET( "ID" ),
      SET( "ID=one\nIdent=two" ), GET( "ID" ), GET( "Ident" ),
      SET( "ID=r\rs" ), GET( "ID" ),
   } },

/* Attributes of an Object that the Rust library has no value for, and the
   Mapping flags of a plain Frame, which it does not store. */
   { "frame2_no_rust_value", "frame2", {
      GET( "Nobject" ), GET( "ObjSize" ), GET( "RefCount" ),
      TEST( "Nobject" ), TEST( "ObjSize" ), TEST( "RefCount" ),
      SET( "Nobject=1" ), SET( "ObjSize=1" ), SET( "RefCount=1" ),
      CLEAR( "Nobject" ), CLEAR( "ObjSize" ), CLEAR( "RefCount" ),
      SET( "Invert=1" ), CLEAR( "Invert" ), SET( "Report=1" ),
      CLEAR( "Report" ),
   } },

/* The same names on a Frame of one axis. */
   { "frame1_object", "frame1", {
      GET( "Class" ), GET( "Nin" ), GET( "Nout" ), SEE( "Invert" ),
      CYCLE( "ID", "ID=x" ), CYCLE( "Ident", "Ident=i" ),
      CYCLE( "UseDefs", "UseDefs=0" ), SET( "Nin=1" ), CLEAR( "Class" ),
   } },

/* Frame attributes on a plain Frame. */
   { "frame_title", "frame2", {
      SEE( "Title" ), CYCLE( "Title", "Title=My title" ),
      SEE( "Domain" ), CYCLE( "Domain", "Domain=my dom" ),
      SEE( "Epoch" ), SET( "Epoch=J2000" ), SEE( "Epoch" ),
      SET( "Epoch=2010-03-01" ), GET( "Epoch" ), SET( "Epoch=B1950" ),
      GET( "Epoch" ), SET( "Epoch=1983.5" ), GET( "Epoch" ),
      SET( "Epoch= 2001.25 " ), GET( "Epoch" ),
      SET( "Epoch=nonsense" ), GET( "Epoch" ), SET( "Epoch=1e20" ),
      GET( "Epoch" ), CLEAR( "Epoch" ), SEE( "Epoch" ),
      SEE( "Naxes" ), SET( "Naxes=3" ), CLEAR( "Naxes" ),
   } },
   { "frame_digits", "frame2", {
      SEE( "Digits" ), CYCLE( "Digits", "Digits=4" ), SET( "Digits=0" ),
      GET( "Digits" ), SET( "Digits=-3" ), GET( "Digits" ),
      SET( "Digits=x" ), SEE( "Digits(1)" ), SET( "Digits(1)=9" ),
      SEE( "Digits(1)" ), SEE( "Digits(2)" ), GET( "Digits" ),
      GET( "Format(1)" ), GET( "Format(2)" ), SET( "Digits(2)=0" ),
      GET( "Digits(2)" ), CLEAR( "Digits(1)" ), SEE( "Digits(1)" ),
      SET( "Digits(3)=2" ), GET( "Digits(3)" ), TEST( "Digits(0)" ),
      CLEAR( "Digits(-1)" ), SET( "Digits( 1 )=6" ), GET( "Digits(+1)" ),
      GET( "Digits(1" ), GET( "Digits(1)x" ), GET( "Digits()" ),
   } },
   { "frame_axis_attrs", "frame2", {
      SEE( "Direction(1)" ), SET( "Direction(1)=0" ), SEE( "Direction(1)" ),
      SET( "Direction(1)=7" ), GET( "Direction(1)" ), CLEAR( "Direction(1)" ),
      SEE( "Direction(1)" ), SET( "Direction(3)=1" ), GET( "Direction(3)" ),
      TEST( "Direction(3)" ), CLEAR( "Direction(3)" ),
      SET( "Direction(1)=abc" ), SET( "Direction=1" ), GET( "Direction" ),
      SEE( "Top(1)" ), CYCLE( "Top(2)", "Top(2)=10.5" ), SEE( "Bottom(2)" ),
      CYCLE( "Bottom(1)", "Bottom(1)=-1e3" ), SET( "Top(3)=1" ),
      GET( "Bottom(0)" ), TEST( "Top(9)" ), CLEAR( "Bottom(9)" ),
      SET( "Top(1)=high" ),
      SEE( "Format(1)" ), CYCLE( "Format(1)", "Format(1)=%6.2f" ),
      GET( "Format(3)" ), SET( "Format(3)=%g" ), TEST( "Format(3)" ),
      CLEAR( "Format(3)" ),
      SEE( "Label(1)" ), CYCLE( "Label(2)", "Label(2)=Y axis" ),
      SET( "Label(3)=x" ), GET( "Label(3)" ), TEST( "Label(3)" ),
      CLEAR( "Label(3)" ), SET( "Label=x" ), GET( "Label" ), TEST( "Label" ),
      CLEAR( "Label" ), SET( "Label(1)=" ), SEE( "Label(1)" ),
      SEE( "Symbol(2)" ), CYCLE( "Symbol(1)", "Symbol(1)=sym" ),
      SET( "Symbol(3)=x" ), GET( "Symbol(3)" ), TEST( "Symbol(3)" ),
      CLEAR( "Symbol(3)" ),
      SEE( "Unit(1)" ), CYCLE( "Unit(1)", "Unit(1)=km" ), SET( "Unit(3)=x" ),
      GET( "Unit(3)" ), TEST( "Unit(3)" ), CLEAR( "Unit(3)" ),
      SEE( "NormUnit(1)" ), SEE( "InternalUnit(1)" ), SET( "Unit(2)=km/s" ),
      GET( "NormUnit(2)" ), GET( "InternalUnit(2)" ), SET( "NormUnit(1)=m" ),
      CLEAR( "NormUnit(1)" ), SET( "InternalUnit(1)=m" ),
      CLEAR( "internalunit(1)" ), SET( "normunitfoo=1" ), SET( "normunit" ),
      CLEAR( "normunit" ), TEST( "normunitxyz" ), CLEAR( "internalunitq" ),
      GET( "NormUnit(3)" ), GET( "NormUnit" ), GET( "InternalUnit(3)" ),
   } },
   { "frame_flags", "frame2", {
      SEE( "MatchEnd" ), CYCLE( "MatchEnd", "MatchEnd=1" ),
      SET( "MatchEnd=7" ), GET( "MatchEnd" ), SET( "MatchEnd=yes" ),
      SEE( "MaxAxes" ), SEE( "MinAxes" ), SET( "MaxAxes=5" ), SEE( "MaxAxes" ),
      SEE( "MinAxes" ), SET( "MinAxes=7" ), SEE( "MinAxes" ), SEE( "MaxAxes" ),
      SET( "MaxAxes=1" ), GET( "MinAxes" ), GET( "MaxAxes" ),
      SET( "MinAxes=-2" ), GET( "MinAxes" ), GET( "MaxAxes" ),
      SET( "MaxAxes=-4" ), GET( "MinAxes" ), GET( "MaxAxes" ),
      CLEAR( "MinAxes,MaxAxes" ), SEE( "MinAxes" ), SEE( "MaxAxes" ),
      SEE( "Permute" ), CYCLE( "Permute", "Permute=0" ),
      SEE( "PreserveAxes" ), CYCLE( "PreserveAxes", "PreserveAxes=1" ),
      SET( "PreserveAxes=-1" ), GET( "PreserveAxes" ),
   } },
   { "frame_system", "frame2", {
      SEE( "System" ), CYCLE( "System", "System=Cartesian" ),
      SET( "System= cartesian " ), SEE( "System" ), SET( "System=nonsense" ),
      SET( "System=two words" ), SET( "System=" ),
      SEE( "AlignSystem" ), CYCLE( "AlignSystem", "AlignSystem=Cartesian" ),
      SET( "AlignSystem=bad" ), SET( "AlignSystem=CART" ),
   } },
   { "frame_observer", "frame2", {
      SEE( "ObsLat" ), SET( "ObsLat=N19:49:31" ), SEE( "ObsLat" ),
      SET( "ObsLat=S19:49:31.5" ), GET( "ObsLat" ), SET( "ObsLat=-30" ),
      GET( "ObsLat" ), SET( "ObsLat=n45 " ), GET( "ObsLat" ),
      SET( "ObsLat=19:49:31 x" ), GET( "ObsLat" ), SET( "ObsLat=bad" ),
      SET( "ObsLat= 10" ), GET( "ObsLat" ), SET( "ObsLat=N" ),
      SET( "ObsLat=" ), CLEAR( "ObsLat" ), SEE( "ObsLat" ),
      SEE( "ObsLon" ), SET( "ObsLon=W155:28:22" ), SEE( "ObsLon" ),
      SET( "ObsLon=E10" ), GET( "ObsLon" ), SET( "ObsLon=200" ),
      GET( "ObsLon" ), SET( "ObsLon=w0:30" ), GET( "ObsLon" ),
      SET( "ObsLon=bad" ), CLEAR( "ObsLon" ), SEE( "ObsLon" ),
      SEE( "ObsAlt" ), CYCLE( "ObsAlt", "ObsAlt=4200" ), SET( "ObsAlt=x" ),
      SET( "ObsAlt=1e3 " ), GET( "ObsAlt" ),
      SEE( "Dtai" ), CYCLE( "Dtai", "Dtai=37" ),
      SEE( "Dut1" ), CYCLE( "Dut1", "Dut1=-0.25" ),
   } },

/* Names with an axis index that Frame does not own go to the Axis object,
   then to the Frame itself without the index. */
   { "frame_axis_path", "frame2", {
      SET( "Ident(2)=x" ), SEE( "Ident(2)" ), SEE( "Ident(1)" ),
      SEE( "Ident" ), SET( "ID(1)=axid" ), SEE( "ID(1)" ), GET( "ID" ),
      SEE( "UseDefs(1)" ), SET( "UseDefs(1)=0" ), SEE( "UseDefs(1)" ),
      GET( "UseDefs" ), CLEAR( "UseDefs(1)" ), SEE( "UseDefs(1)" ),
      CLEAR( "ID(1)" ), SEE( "ID(1)" ),
      SEE( "Class(1)" ), SET( "Class(1)=x" ), CLEAR( "Class(2)" ),
      SET( "Fred(1)=2" ), GET( "Fred(1)" ), TEST( "Fred(1)" ),
      CLEAR( "Fred(1)" ), SET( "Fred(3)=2" ), GET( "Fred(3)" ),
      TEST( "Fred(3)" ), CLEAR( "Fred(3)" ), GET( "Fred(1" ),
      SET( "Fred(1)x=2" ), SET( "Fred(x)=2" ), GET( "Fred(x)" ),
      GET( "Nin(1)" ), TEST( "Nin(1)" ), SET( "Nin(1)=4" ), CLEAR( "Nin(2)" ),
      SET( "Title(1)=via axis" ), SEE( "Title(2)" ), SEE( "Title" ),
      CLEAR( "Title(1)" ), SEE( "Title" ), GET( "Naxes(1)" ),
      SET( "Epoch(2)=2001" ), GET( "Epoch(1)" ), GET( "Epoch" ),
      GET( "IsLatitude(1)" ), SET( "AsTime(1)=1" ), GET( "Domain(2)" ),
      SET( "Label(1)(2)=x" ), GET( "Label(1)(2)" ),
      SET( "Direction(1)=abc" ),
   } },

/* A dump with one of everything set. */
   { "frame_dump", "frame2", {
      SET( "Title=T,Domain=D,Epoch=2020,Digits=5,Digits(1)=3,Direction(2)=0,"
           "Format(2)=%.3e,Label(1)=L,Symbol(2)=s,Unit(2)=m/s,Top(2)=4,"
           "Bottom(1)=-1,MatchEnd=1,MaxAxes=4,MinAxes=2,Permute=0,"
           "PreserveAxes=1,ObsLat=-30,ObsLon=120,ObsAlt=100,Dtai=36,"
           "Dut1=-0.1,System=Cartesian,AlignSystem=Cartesian,"
           "Ident(2)=axis two,UseDefs(1)=0,Ident=whole,UseDefs=0" ),
   } },

/* Dates C's reader rejects with a message of its own, and other forms. */
   { "frame_epoch_forms", "frame2", {
      SET( "Epoch=2010-Xyz-01" ), SET( "Epoch=2010-03-01 25:00" ),
      SET( "Epoch=2010-03-01 10:61" ), SET( "Epoch=2010-03-01 10:10:61.5" ),
      GET( "Epoch" ), SET( "Epoch=2010-03-45" ), GET( "Epoch" ),
      SET( "Epoch=MJD 55000" ), GET( "Epoch" ), SET( "Epoch=JD 2455000.5" ),
      GET( "Epoch" ), SET( "Epoch=1996-Oct-2 12:30" ), GET( "Epoch" ),
      SET( "Epoch=1996-10-2T12:30:10.5" ), GET( "Epoch" ),
      SET( "Epoch=b1975.5" ), GET( "Epoch" ),
   } },

/* The SkyAxis of an axis picked from a SkyFrame into a plain Frame. */
   { "frame_skyaxis", "skyaxis1", {
      GET( "Class" ), GET( "Class(1)" ), SEE( "IsLatitude(1)" ),
      SEE( "CentreZero(1)" ), SEE( "AsTime(1)" ), SET( "IsLatitude(1)=1" ),
      SEE( "IsLatitude(1)" ), SEE( "CentreZero(1)" ), SEE( "AsTime(1)" ),
      SET( "CentreZero(1)=0" ), SEE( "CentreZero(1)" ), SET( "AsTime(1)=5" ),
      SEE( "AsTime(1)" ), GET( "Format(1)" ), GET( "Label(1)" ),
      GET( "Unit(1)" ), CLEAR( "IsLatitude(1)" ), SEE( "IsLatitude(1)" ),
      CLEAR( "AsTime(1)" ), SEE( "AsTime(1)" ), SET( "IsLatitude(1)=0" ),
      GET( "AsTime(1)" ), CLEAR( "CentreZero(1)" ), SEE( "CentreZero(1)" ),
      SET( "Fred(1)=1" ), GET( "Fred(1)" ), SET( "IsLatitude=1" ),
      GET( "IsLatitude" ), SET( "IsLatitude(1)=x" ), SET( "Ident(1)=sky" ),
      GET( "Ident(1)" ), SET( "AsTime(1)=1" ),
   } },

/* A Frame of one axis takes an axis attribute without its index. */
   { "frame_one_axis", "frame1", {
      SET( "Label=x" ), SEE( "Label" ), GET( "Label(1)" ), CLEAR( "Label" ),
      SEE( "Label" ), SET( "Unit=km" ), SEE( "Unit" ), GET( "NormUnit" ),
      GET( "InternalUnit" ), TEST( "NormUnit" ), SET( "NormUnit=x" ),
      CLEAR( "NormUnit" ), SET( "Top=5" ), SEE( "Top" ), SET( "Bottom=-1" ),
      SEE( "Bottom" ), SET( "Direction=0" ), SEE( "Direction" ),
      SET( "Format=%5.1f" ), SEE( "Format" ), SET( "Symbol=s" ),
      SEE( "Symbol" ), SET( "Digits=3" ), SEE( "Digits" ), SEE( "Digits(1)" ),
      SET( "Fred=1" ), GET( "Fred" ), TEST( "Fred" ), CLEAR( "Fred" ),
      SET( "Label(2)=x" ), GET( "Label(2)" ), SEE( "Ident(1)" ),
      SET( "Ident=whole" ), GET( "Ident" ), GET( "Ident(1)" ),
      SET( "Label=" ), GET( "Label" ), SET( "Top=high" ), SET( "Label" ),
      GET( "Title" ), SET( "Title=one" ), GET( "Title" ),
   } },

/* SkyFrame's own attributes. */
   { "sky_own", "sky", {
      SEE( "AsTime(1)" ), SEE( "AsTime(2)" ), CYCLE( "AsTime(2)", "AsTime(2)=1" ),
      SET( "AsTime(1)=0" ), SEE( "AsTime(1)" ), GET( "Format(1)" ),
      GET( "Unit(1)" ), CLEAR( "AsTime(1)" ), SET( "AsTime(3)=1" ),
      GET( "AsTime(3)" ), TEST( "AsTime(0)" ), CLEAR( "AsTime(3)" ),
      GET( "AsTime" ), SET( "AsTime=1" ), SET( "AsTime(1)=x" ),
      SEE( "Equinox" ), SET( "Equinox=B1950" ), SEE( "Equinox" ),
      SET( "Equinox=J2010.5" ), GET( "Equinox" ), SET( "Equinox=1975" ),
      GET( "Equinox" ), SET( "Equinox=2001-01-01" ), GET( "Equinox" ),
      SET( "Equinox=nonsense" ), SET( "Equinox=2001-01-01 25:00" ),
      CLEAR( "Equinox" ), SEE( "Equinox" ),
      SEE( "NegLon" ), CYCLE( "NegLon", "NegLon=1" ), SET( "NegLon=-3" ),
      GET( "NegLon" ), CLEAR( "NegLon" ),
      SEE( "SkyTol" ), CYCLE( "SkyTol", "SkyTol=0.01" ), SET( "SkyTol=x" ),
      SEE( "Projection" ), CYCLE( "Projection", "Projection=gnomonic tan" ),
      SEE( "AlignOffset" ), CYCLE( "AlignOffset", "AlignOffset=1" ),
      SET( "AlignOffset=9" ), GET( "AlignOffset" ),
      SEE( "LatAxis" ), SEE( "LonAxis" ), SEE( "IsLatAxis(1)" ),
      SEE( "IsLatAxis(2)" ), SEE( "IsLonAxis(1)" ), GET( "IsLatAxis(3)" ),
      GET( "IsLonAxis(0)" ), SET( "LatAxis=1" ), SET( "LonAxis=2" ),
      SET( "IsLatAxis(1)=1" ), SET( "islonaxisfoo" ), CLEAR( "LatAxis" ),
      CLEAR( "LonAxis" ), CLEAR( "IsLatAxis(1)" ), CLEAR( "islonaxis" ),
      TEST( "islataxisxyz" ), GET( "IsLatAxis" ), GET( "islonaxis" ),
   } },
   { "sky_system", "sky", {
      SEE( "System" ), SET( "System=Galactic" ), SEE( "System" ),
      GET( "Label(1)" ), GET( "Title" ), SET( "System=FK4" ), GET( "System" ),
      GET( "Equinox" ), GET( "Epoch" ), GET( "Title" ),
      SET( "System=nonsense" ), SET( "System=fk5 " ), GET( "System" ),
      SET( "System=ICRS" ), GET( "System" ), SET( "System=ecliptic" ),
      GET( "System" ), SET( "System=AzEl" ), GET( "System" ),
      SET( "System=unknown" ), GET( "System" ), SET( "System=Cartesian" ),
      SET( "System=SUPERGALACTIC" ), GET( "System" ),
      SET( "System=HELIOECLIPTIC" ), GET( "System" ), SET( "System=GAPPT" ),
      GET( "System" ), SET( "System=APPARENT" ), GET( "System" ),
      SET( "System=J2000" ), GET( "System" ), SET( "System=FK4-NO-E" ),
      GET( "System" ), SET( "System=FK4_NO_E" ), GET( "System" ),
      SET( "System=EQUATORIAL" ), GET( "System" ), SET( "System=GEOCENTRIC" ),
      GET( "System" ), SET( "System=FK5" ), CLEAR( "System" ), SEE( "System" ),
      SEE( "AlignSystem" ), CYCLE( "AlignSystem", "AlignSystem=FK5" ),
      SET( "AlignSystem=bad" ), SET( "AlignSystem=galactic" ),
      GET( "AlignSystem" ),
   } },
   { "sky_ref", "sky", {
      SEE( "SkyRef" ), SEE( "SkyRef(1)" ), SEE( "SkyRefIs" ), SEE( "SkyRefP" ),
      SEE( "SkyRefP(2)" ),
      SET( "SkyRef=12:30:00,-30:00:00" ), SEE( "SkyRef" ), GET( "SkyRef(1)" ),
      GET( "SkyRef(2)" ), TEST( "SkyRef(1)" ), SET( "SkyRef(2)=0.5" ),
      GET( "SkyRef" ), GET( "SkyRef(2)" ),
      SET( "SkyRef= 1:00:00 , 10:00:00 " ), GET( "SkyRef" ),
      SET( "SkyRef=12:30:00" ), SET( "SkyRef=12:30:00;10:00" ),
      SET( "SkyRef=bad,bad" ), SET( "SkyRef=1:00:00,10:00:00 x" ),
      GET( "SkyRef" ), SET( "SkyRef(3)=0.1" ), GET( "SkyRef(3)" ),
      TEST( "SkyRef(0)" ), CLEAR( "SkyRef(3)" ), SET( "SkyRef(1)=x" ),
      SET( "SkyRefIs=Origin" ), SEE( "SkyRefIs" ), SET( "SkyRefIs=pole" ),
      GET( "SkyRefIs" ), SET( "SkyRefIs=ignored " ), GET( "SkyRefIs" ),
      SET( "SkyRefIs=bad" ), SET( "SkyRefIs= Pole" ), GET( "SkyRefIs" ),
      CLEAR( "SkyRefIs" ), SEE( "SkyRefIs" ),
      SET( "SkyRefP=0:00:00,80:00:00" ), SEE( "SkyRefP" ), GET( "SkyRefP(1)" ),
      GET( "SkyRefP(2)" ), SET( "SkyRefP(1)=0.25" ), GET( "SkyRefP" ),
      SET( "SkyRefP=junk" ), SET( "SkyRefP(4)=1" ), GET( "SkyRefP(4)" ),
      TEST( "SkyRefP(4)" ), CLEAR( "SkyRefP(4)" ), CLEAR( "SkyRefP(1)" ),
      SEE( "SkyRefP(1)" ), SEE( "SkyRefP" ), CLEAR( "SkyRefP" ),
      SEE( "SkyRefP" ),
      CLEAR( "SkyRef(1)" ), SEE( "SkyRef" ), SEE( "SkyRef(1)" ),
      SEE( "SkyRef(2)" ), CLEAR( "SkyRef" ), SEE( "SkyRef" ),
      SET( "Digits=10" ), SET( "SkyRef=1:02:03.456,7:08:09.12" ),
      GET( "SkyRef" ), SET( "Format(1)=dms.1" ), GET( "SkyRef" ),
   } },

/* Names with an axis index on a SkyFrame, and the Frame names it inherits. */
   { "sky_axis_path", "sky", {
      SET( "Equinox(1)=J2010" ), GET( "Equinox" ), GET( "Equinox(2)" ),
      TEST( "Equinox(1)" ), CLEAR( "Equinox(2)" ), SEE( "Equinox" ),
      SEE( "IsLatitude(1)" ), SEE( "IsLatitude(2)" ), SET( "IsLatitude(2)=0" ),
      SEE( "IsLatitude(2)" ), CLEAR( "IsLatitude(2)" ), SEE( "CentreZero(1)" ),
      SEE( "CentreZero(2)" ), SET( "CentreZero(1)=1" ), SEE( "CentreZero(1)" ),
      GET( "Class(1)" ), GET( "Class(2)" ), SET( "Ident(1)=lon" ),
      GET( "Ident(1)" ), SET( "Fred(1)=1" ), GET( "Fred(2)" ),
      SET( "NegLon(1)=1" ), GET( "NegLon" ), GET( "System(1)" ),
      SET( "System(2)=Galactic" ), GET( "System" ), CLEAR( "System(1)" ),
      SEE( "Label(1)" ), CYCLE( "Label(2)", "Label(2)=Dec label" ),
      GET( "Symbol(1)" ), GET( "Symbol(2)" ), GET( "Unit(2)" ),
      GET( "Format(1)" ), SET( "Format(1)=dms.3" ), GET( "Format(1)" ),
      GET( "Unit(1)" ), TEST( "Format(1)" ), CLEAR( "Format(1)" ),
      GET( "Direction(1)" ), GET( "Direction(2)" ), GET( "Top(2)" ),
      GET( "Bottom(2)" ), GET( "Top(1)" ), SEE( "Title" ), SEE( "Domain" ),
      SEE( "Epoch" ), GET( "Digits" ), GET( "Digits(1)" ),
      GET( "NormUnit(1)" ), GET( "InternalUnit(1)" ), GET( "InternalUnit(2)" ),
      GET( "Naxes" ), GET( "Nin" ), GET( "Class" ), GET( "MaxAxes" ),
      GET( "MinAxes" ), GET( "ObsLat" ), SET( "Label(3)=x" ),
      SET( "Epoch=1990.5" ), GET( "Epoch" ), GET( "Title" ),
      SET( "Fred=1" ), GET( "Fred" ), TEST( "Fred" ), CLEAR( "Fred" ),
   } },
   { "sky_perm", "sky_perm", {
      GET( "LatAxis" ), GET( "LonAxis" ), GET( "IsLatAxis(1)" ),
      GET( "IsLonAxis(1)" ), SET( "AsTime(1)=1" ), GET( "AsTime(1)" ),
      GET( "AsTime(2)" ), SET( "SkyRef=-30:00:00,12:30:00" ), GET( "SkyRef" ),
      GET( "SkyRef(1)" ), GET( "SkyRef(2)" ), SET( "SkyRef(1)=0.1" ),
      GET( "SkyRef" ), GET( "Label(1)" ), GET( "Label(2)" ),
      SET( "IsLatitude(1)=1" ), GET( "IsLatitude(1)" ), GET( "IsLatitude(2)" ),
      SET( "Ident(1)=first" ), GET( "Ident(1)" ), GET( "Ident(2)" ),
      GET( "Class(1)" ), GET( "Format(1)" ), GET( "Format(2)" ),
      SET( "SkyRefP=80:00:00,0:00:00" ), GET( "SkyRefP" ),
      GET( "SkyRefP(1)" ), SET( "Digits(1)=9" ), GET( "Digits(1)" ),
      GET( "Digits(2)" ), GET( "Format(1)" ),
   } },
   { "sky_dump", "sky", {
      SET( "System=FK4,Equinox=B1975,Epoch=1980,NegLon=1,SkyTol=0.002,"
           "Projection=tan,SkyRef=\"1:00:00,10:00:00\",SkyRefIs=Origin,"
           "SkyRefP=\"2:00:00,80:00:00\",AlignOffset=1,AsTime(2)=1,"
           "Label(1)=LON,Digits(2)=9,Ident(2)=lat axis,CentreZero(1)=1,"
           "Title=sky,AlignSystem=ICRS,Format(1)=hms.2" ),
   } },

/* SpecFrame's own attributes. */
   { "spec_system", "spec", {
      SEE( "System" ), SET( "System=wave" ), SEE( "System" ), GET( "Unit" ),
      GET( "Label" ), SET( "System=FREQ" ), GET( "System" ),
      SET( "System=vrad" ), GET( "System" ), GET( "Unit" ),
      SET( "System=nonsense" ), SET( "System=ENER" ), GET( "System" ),
      SET( "System=VOPT" ), GET( "System" ), SET( "System=zopt" ),
      GET( "System" ), SET( "System=beta" ), GET( "System" ),
      SET( "System=VELO" ), GET( "System" ), SET( "System=awav" ),
      GET( "System" ), SET( "System=wavn" ), GET( "System" ),
      SET( "System=VRADIO" ), GET( "System" ), SET( "System=redshift" ),
      GET( "System" ), SET( "System=Cartesian" ), CLEAR( "System" ),
      SEE( "System" ), SEE( "AlignSystem" ),
      CYCLE( "AlignSystem", "AlignSystem=freq" ), SET( "AlignSystem=bad" ),
      SEE( "StdOfRest" ), SET( "StdOfRest=LSRK" ), SEE( "StdOfRest" ),
      SET( "StdOfRest=topo" ), GET( "StdOfRest" ), SET( "StdOfRest=nonsense" ),
      SET( "StdOfRest= LSRD" ), GET( "StdOfRest" ), SET( "StdOfRest=LSRD " ),
      GET( "StdOfRest" ), SET( "StdOfRest=geo" ), GET( "StdOfRest" ),
      SET( "StdOfRest=BARY" ), GET( "StdOfRest" ), SET( "StdOfRest=helio" ),
      GET( "StdOfRest" ), SET( "StdOfRest=galactic" ), GET( "StdOfRest" ),
      SET( "StdOfRest=Local_group" ), GET( "StdOfRest" ),
      SET( "StdOfRest=source" ), GET( "StdOfRest" ), SET( "StdOfRest=LSR" ),
      GET( "StdOfRest" ), CLEAR( "StdOfRest" ), SEE( "StdOfRest" ),
      SEE( "AlignStdOfRest" ), CYCLE( "AlignStdOfRest", "AlignStdOfRest=bary" ),
      SET( "AlignStdOfRest=bad" ), SET( "AlignStdOfRest= bary" ),
      SEE( "AlignSpecOffset" ), CYCLE( "AlignSpecOffset", "AlignSpecOffset=1" ),
   } },
   { "spec_values", "spec", {
      SEE( "RestFreq" ), SET( "RestFreq=230.5" ), SEE( "RestFreq" ),
      SET( "RestFreq=1.3 mm" ), GET( "RestFreq" ), SET( "RestFreq=345 GHz" ),
      GET( "RestFreq" ), SET( "RestFreq=1e-3 eV" ), GET( "RestFreq" ),
      SET( "RestFreq=1 kg" ), SET( "RestFreq=0 m" ), SET( "RestFreq=x" ),
      SET( "RestFreq=5 nonsense" ), SET( "RestFreq= 115.27 " ),
      GET( "RestFreq" ), CLEAR( "RestFreq" ), SEE( "RestFreq" ),
      SEE( "SourceVel" ), SEE( "SourceSys" ), SET( "SourceVel=20" ),
      SEE( "SourceVel" ), SET( "SourceSys=VRAD" ), SEE( "SourceSys" ),
      GET( "SourceVel" ), SET( "SourceVel=30" ), GET( "SourceVel" ),
      SET( "SourceSys=ZOPT" ), GET( "SourceVel" ), SET( "SourceSys=bad" ),
      GET( "SourceSys" ), SET( "SourceSys=FREQ" ), GET( "SourceSys" ),
      CLEAR( "SourceSys" ), SEE( "SourceSys" ), CLEAR( "SourceVel" ),
      SEE( "SourceVel" ),
      SEE( "SourceVRF" ), CYCLE( "SourceVRF", "SourceVRF=LSRK" ),
      SET( "SourceVRF=bad" ),
      SEE( "SpecOrigin" ), SET( "SpecOrigin=230 GHz" ), SEE( "SpecOrigin" ),
      SET( "SpecOrigin=100" ), GET( "SpecOrigin" ), SET( "Unit=MHz" ),
      GET( "SpecOrigin" ), SET( "SpecOrigin=5 km/s" ), SET( "SpecOrigin=x" ),
      GET( "SpecOrigin" ), CLEAR( "SpecOrigin" ), SEE( "SpecOrigin" ),
      SEE( "RefRA" ), SEE( "RefDec" ), SET( "RefRA=10:00:00" ), SEE( "RefRA" ),
      SET( "RefDec=-20:30:00" ), SEE( "RefDec" ), SET( "RefRA=bad" ),
      SET( "RefDec=bad" ), SET( "RefRA=10:00:00 x" ), SET( "RefRA= 1:00" ),
      GET( "RefRA" ), SET( "RefDec=45" ), GET( "RefDec" ), CLEAR( "RefRA" ),
      CLEAR( "RefDec" ), SEE( "RefRA" ), SEE( "RefDec" ),
      SEE( "GeoLat" ), SET( "GeoLat=N19:49:31" ), SEE( "GeoLat" ),
      GET( "ObsLat" ), SEE( "GeoLon" ), SET( "GeoLon=W155:28:22" ),
      SEE( "GeoLon" ), GET( "ObsLon" ), SET( "GeoLat=bad" ),
      CLEAR( "GeoLat" ), SEE( "GeoLat" ), CLEAR( "GeoLon" ), SEE( "ObsLon" ),
   } },

/* The axis attributes of a SpecFrame without an index, and what it
   inherits. A Top, Bottom or Direction without an index and with a value
   that is not a number is left out: C's SetAttrib functions call each other
   without end for it (c-library-quirks.md) and the probe would crash. */
   { "spec_axis", "spec", {
      SET( "Unit=GHz" ), SEE( "Unit" ), GET( "Unit(1)" ), SET( "Label=Freq" ),
      SEE( "Label" ), SET( "Symbol=f" ), SEE( "Symbol" ),
      SET( "Format=%.3f" ), SEE( "Format" ), SET( "Direction=0" ),
      SEE( "Direction" ), SET( "Top=500" ), SEE( "Top" ), SET( "Bottom=1" ),
      SEE( "Bottom" ), CLEAR( "Unit" ), SEE( "Unit" ), CLEAR( "Label" ),
      SEE( "Label" ), CLEAR( "Symbol" ), SEE( "Symbol" ), CLEAR( "Format" ),
      SEE( "Format" ), CLEAR( "Direction" ), CLEAR( "Top" ), CLEAR( "Bottom" ),
      SEE( "Top" ), SET( "Unit(1)=MHz" ), GET( "Unit" ),
      SET( "Unit=furlongs" ), GET( "Unit" ), SET( "Unit(2)=x" ),
      SET( "Label(1)=L1" ),
      GET( "Label(1)" ), SEE( "Digits" ), SET( "Digits=5" ),
      GET( "Digits(1)" ), GET( "Class" ), GET( "Class(1)" ),
      SET( "Ident(1)=ax" ), GET( "Ident(1)" ), GET( "Naxes" ), GET( "Title" ),
      GET( "Domain" ), SET( "Epoch=2001" ), GET( "Epoch" ), SET( "Fred=1" ),
      GET( "Fred" ), TEST( "Fred" ), CLEAR( "Fred" ), GET( "NormUnit" ),
      GET( "InternalUnit" ), SET( "System(1)=wave" ), GET( "System" ),
      GET( "Nin" ), GET( "MaxAxes" ), SET( "Unit=" ), GET( "Unit" ),
   } },
   { "spec_dump", "spec", {
      SET( "System=VRAD,StdOfRest=LSRK,RestFreq=230.538,SourceVel=10,"
           "SourceVRF=BARY,SourceSys=VRAD,RefRA=10:00:00,RefDec=20:00:00,"
           "SpecOrigin=5,AlignSpecOffset=1,AlignStdOfRest=Topo,"
           "AlignSystem=FREQ,Unit=km/s,Label=Vel,Epoch=2005,ObsLat=N20,"
           "GeoLon=W155,Ident=sp,Title=A spectrum" ),
   } },

/* DSBSpecFrame's own attributes, and SpecFrame's through it. */
   { "dsb_own", "dsb", {
      SEE( "DSBCentre" ), SET( "DSBCentre=230" ), SEE( "DSBCentre" ),
      SEE( "IF" ), SET( "IF=4" ), SEE( "IF" ), SEE( "ImagFreq" ),
      SET( "IF=4 GHz" ), GET( "IF" ), SET( "IF=4000 MHz" ), GET( "IF" ),
      SET( "IF=-4" ), GET( "IF" ), GET( "ImagFreq" ), SET( "IF=3 m" ),
      SET( "IF=x" ), SEE( "SideBand" ), SET( "SideBand=lsb" ),
      SEE( "SideBand" ), GET( "ImagFreq" ), SET( "SideBand=USB" ),
      GET( "SideBand" ), SET( "SideBand=lo" ), GET( "SideBand" ),
      SET( "SideBand=observed" ), GET( "SideBand" ), SET( "SideBand=image" ),
      GET( "SideBand" ), SET( "SideBand=bad" ), SET( "SideBand= usb " ),
      GET( "SideBand" ), CLEAR( "SideBand" ), SEE( "SideBand" ),
      SEE( "AlignSideBand" ), CYCLE( "AlignSideBand", "AlignSideBand=1" ),
      SET( "DSBCentre=230 GHz" ), GET( "DSBCentre" ),
      SET( "DSBCentre=1.3 mm" ), SET( "DSBCentre=x" ),
      SET( "DSBCentre=230000 MHz" ), GET( "DSBCentre" ),
      SET( "ImagFreq=1" ), CLEAR( "ImagFreq" ), TEST( "ImagFreq" ),
      CLEAR( "DSBCentre" ), SEE( "DSBCentre" ), CLEAR( "IF" ), SEE( "IF" ),
      SET( "RestFreq=230.5" ), GET( "RestFreq" ), SET( "DSBCentre=231" ),
      SET( "System=wave" ), GET( "DSBCentre" ), SET( "DSBCentre=0.0013" ),
      GET( "DSBCentre" ), GET( "Class" ), SET( "Fred=1" ), GET( "Fred" ),
      SET( "Label=LSB freq" ), GET( "Label" ), SET( "StdOfRest=LSRK" ),
      GET( "StdOfRest" ), SET( "GeoLat=N19" ), GET( "ObsLat" ),
      GET( "Unit" ), GET( "Title" ),
   } },
   { "dsb_dump", "dsb", {
      SET( "DSBCentre=345,IF=-5,SideBand=USB,AlignSideBand=1,RestFreq=345.8,"
           "StdOfRest=LSRK,Title=dsb" ),
   } },

/* TimeFrame's own attributes. */
   { "time_own", "time", {
      SEE( "TimeScale" ), SET( "TimeScale=TAI" ), SEE( "TimeScale" ),
      SET( "TimeScale=bad" ), SET( "TimeScale=utc " ), GET( "TimeScale" ),
      SET( "TimeScale= TT" ), SET( "TimeScale=TT" ), GET( "TimeScale" ),
      SET( "TimeScale=TDB" ), GET( "TimeScale" ), SET( "TimeScale=TCB" ),
      GET( "TimeScale" ), SET( "TimeScale=TCG" ), GET( "TimeScale" ),
      SET( "TimeScale=UT1" ), GET( "TimeScale" ), SET( "TimeScale=LAST" ),
      GET( "TimeScale" ), SET( "TimeScale=LMST" ), GET( "TimeScale" ),
      SET( "TimeScale=GMST" ), GET( "TimeScale" ), SET( "TimeScale=LT" ),
      GET( "TimeScale" ), CLEAR( "TimeScale" ), SEE( "TimeScale" ),
      SEE( "AlignTimeScale" ), CYCLE( "AlignTimeScale", "AlignTimeScale=TDB" ),
      SET( "AlignTimeScale=bad" ),
      SEE( "System" ), SET( "System=JD" ), SEE( "System" ), GET( "Unit" ),
      SET( "System=JEPOCH" ), GET( "System" ), GET( "Unit" ),
      SET( "System=BEPOCH" ), GET( "System" ), SET( "System=MJD" ),
      GET( "System" ), SET( "System=nonsense" ), SET( "System=Julian" ),
      CLEAR( "System" ), SEE( "System" ), SEE( "AlignSystem" ),
      CYCLE( "AlignSystem", "AlignSystem=JD" ), SET( "AlignSystem=bad" ),
      SEE( "LTOffset" ), CYCLE( "LTOffset", "LTOffset=-10" ),
      SET( "LTOffset=x" ),
      SEE( "ClockLat" ), SET( "ClockLat=N19" ), SEE( "ClockLat" ),
      GET( "ObsLat" ), SEE( "ClockLon" ), SET( "ClockLon=W155:28" ),
      SEE( "ClockLon" ), GET( "ObsLon" ), SET( "ClockLat=bad" ),
      CLEAR( "ClockLat" ), SEE( "ClockLat" ), CLEAR( "ClockLon" ),
      SEE( "ObsLon" ),
   } },
   { "time_origin", "time", {
      SEE( "TimeOrigin" ), SET( "TimeOrigin=55197" ), SEE( "TimeOrigin" ),
      SET( "TimeOrigin=2010-01-01" ), GET( "TimeOrigin" ),
      SET( "TimeOrigin=55198 d" ), GET( "TimeOrigin" ),
      SET( "TimeOrigin=150 yr" ), GET( "TimeOrigin" ),
      SET( "TimeOrigin=24 h" ), GET( "TimeOrigin" ), SET( "TimeOrigin=5 km" ),
      SET( "TimeOrigin=nonsense" ), SET( "TimeOrigin=1996-Oct-2 12:30" ),
      GET( "TimeOrigin" ), SET( "TimeOrigin=J2000" ), GET( "TimeOrigin" ),
      SET( "TimeOrigin=2010 Jan" ), SET( "TimeOrigin=2010 Jan 5" ),
      GET( "TimeOrigin" ), SET( "TimeOrigin=2010-01-01 25:00" ),
      SET( "Unit=h" ), GET( "Unit" ), SET( "TimeOrigin=24" ),
      GET( "TimeOrigin" ), CLEAR( "Unit" ), GET( "TimeOrigin" ),
      SET( "System=JEPOCH" ), GET( "TimeOrigin" ),
      SET( "TimeOrigin=2010-01-01" ), GET( "TimeOrigin" ),
      SET( "System=JD" ), GET( "TimeOrigin" ), SET( "TimeOrigin=B1950" ),
      GET( "TimeOrigin" ), CLEAR( "TimeOrigin" ), SEE( "TimeOrigin" ),
   } },

/* The axis attributes of a TimeFrame without an index, and what it
   inherits. Top, Bottom and Direction with a value that is not a number
   are left out, as for a SpecFrame. */
   { "time_axis", "time", {
      SET( "Unit=s" ), SEE( "Unit" ), SET( "Label=Time" ), SEE( "Label" ),
      SET( "Symbol=t" ), SEE( "Symbol" ), SET( "Format=iso.2" ),
      SEE( "Format" ), SET( "Direction=0" ), SEE( "Direction" ),
      SET( "Top=60000" ), SEE( "Top" ), SET( "Bottom=50000" ), SEE( "Bottom" ),
      CLEAR( "Format" ), SEE( "Format" ), CLEAR( "Unit" ), SEE( "Unit" ),
      CLEAR( "Label" ), SEE( "Label" ), GET( "Class" ), GET( "Class(1)" ),
      GET( "Title" ), GET( "Domain" ), GET( "Naxes" ), SET( "Fred=1" ),
      GET( "Fred" ), TEST( "Fred" ), CLEAR( "Fred" ), SET( "Epoch=2001" ),
      GET( "Epoch" ), GET( "Digits" ), SET( "Unit(2)=x" ), GET( "NormUnit" ),
      GET( "InternalUnit" ), SET( "System=BEPOCH" ), SET( "Unit=d" ),
      GET( "Unit" ), SET( "Ident(1)=t axis" ), GET( "Ident(1)" ),
      GET( "Nout" ),
   } },
   { "time_dump", "time", {
      SET( "System=JD,TimeScale=TAI,TimeOrigin=2455000,LTOffset=-10,"
           "AlignTimeScale=UTC,AlignSystem=MJD,Unit=d,Label=T,ClockLat=N20,"
           "ObsLon=W155,Title=A time,Ident=tf" ),
   } },

/* A CmpFrame of a SpecFrame and a SkyFrame: names go to the CmpFrame as a
   Frame first, then to the Frames its axes belong to. "Top(1)=high" is left
   out: it reaches the SpecFrame, where C never returns. */
   { "cmp_spec_sky", "cmp_spec_sky", {
      GET( "Class" ), GET( "Naxes" ), GET( "Title" ), GET( "Domain" ),
      SET( "System(1)=wave" ), GET( "System(1)" ), GET( "System" ),
      GET( "System(2)" ), SET( "System=FK4" ), GET( "System(2)" ),
      GET( "System(3)" ), GET( "System" ), SET( "System=nonsense" ),
      SET( "System(2)=Galactic" ), GET( "System(3)" ), TEST( "System(1)" ),
      TEST( "System" ), CLEAR( "System(2)" ), GET( "System(2)" ),
      SET( "Label(2)=x" ), GET( "Label(2)" ), SEE( "Label(1)" ),
      GET( "Label(3)" ), SET( "Label(4)=x" ), GET( "Label(4)" ),
      TEST( "Label(4)" ), CLEAR( "Label(4)" ), GET( "Unit(1)" ),
      GET( "Unit(2)" ), GET( "Format(2)" ), SET( "Epoch=2000" ),
      SEE( "Epoch" ), GET( "Epoch(1)" ), SET( "RestFreq=230.5" ),
      GET( "RestFreq" ), TEST( "RestFreq" ), SET( "RestFreq=bad" ),
      SET( "Fred=1" ), GET( "Fred" ), TEST( "Fred" ), CLEAR( "Fred" ),
      SET( "Fred(1)=1" ), GET( "Fred(1)" ), TEST( "Fred(1)" ),
      CLEAR( "Fred(1)" ), SET( "Fred(9)=1" ), GET( "Fred(9)" ),
      TEST( "Fred(9)" ), CLEAR( "Fred(9)" ),
      SET( "Equinox=B1950" ), GET( "Equinox" ), GET( "Equinox(2)" ),
      GET( "Equinox(1)" ), SET( "StdOfRest=LSRK" ), SEE( "StdOfRest" ),
      CLEAR( "StdOfRest" ), SEE( "StdOfRest" ), SET( "SkyRef(2)=0.5" ),
      GET( "SkyRef(2)" ), GET( "SkyRef" ), SET( "AsTime(3)=1" ),
      GET( "AsTime(3)" ), GET( "Digits" ), SET( "Digits=5" ),
      GET( "Digits(2)" ), SET( "Digits(2)=8" ), GET( "Digits(2)" ),
      GET( "Format(2)" ), SET( "Ident(2)=axis" ), GET( "Ident(2)" ),
      GET( "Class(1)" ), GET( "Class(2)" ), SET( "Title=compound" ),
      GET( "Title" ), GET( "Title(1)" ), SET( "Domain=CMP" ), GET( "Domain" ),
      GET( "Domain(2)" ), SET( "ObsLat=N19" ), GET( "ObsLat" ),
      GET( "ObsLat(1)" ), GET( "Nin" ), GET( "MaxAxes" ), GET( "LatAxis" ),
      GET( "LonAxis" ), GET( "IsLatAxis(3)" ), SET( "Projection=a b" ),
      GET( "Projection" ), SET( "Label=every" ), GET( "Label(1)" ),
      GET( "Label(2)" ), GET( "Label(3)" ), CLEAR( "Label" ),
      GET( "Label(1)" ), TEST( "Label" ), GET( "Label" ), SET( "Unit=GHz" ),
      GET( "Unit(1)" ), GET( "Unit(2)" ), SET( "NegLon=1" ), SEE( "NegLon" ),
      CLEAR( "NegLon" ), SEE( "NegLon" ), SET( "Nin=3" ), CLEAR( "Naxes" ),
   } },

/* A CmpFrame of two plain Frames of one axis. */
   { "cmp_1_1", "cmp_1_1", {
      GET( "Label(1)" ), GET( "Label(2)" ), SET( "Label(2)=x" ),
      SEE( "Label(2)" ), CLEAR( "Label(2)" ), SEE( "Label(2)" ),
      GET( "Symbol(2)" ), GET( "Unit(2)" ), SET( "Digits(2)=4" ),
      SEE( "Digits(2)" ), GET( "Format(2)" ), GET( "Title" ),
      SET( "Title=t" ), GET( "Title" ), GET( "Domain" ), SET( "Domain=A" ),
      GET( "Domain(1)" ), SEE( "System" ), SET( "System=Cartesian" ),
      SEE( "System" ), GET( "System(1)" ), SET( "System=Compound" ),
      SEE( "System" ), CLEAR( "System" ), SEE( "AlignSystem" ),
      SET( "AlignSystem=Compound" ), SEE( "AlignSystem" ), SET( "Top(2)=5" ),
      GET( "Top(2)" ), GET( "Top(1)" ), SET( "Fred(1)=1" ), GET( "Fred(1)" ),
      TEST( "Fred(1)" ), CLEAR( "Fred(1)" ), SET( "Fred(3)=1" ),
      GET( "Fred(3)" ), TEST( "Fred(3)" ), CLEAR( "Fred(3)" ),
      GET( "Naxes" ), GET( "Ident(1)" ), SET( "Ident=whole" ), GET( "Ident" ),
      SET( "Ident(1)=ax1" ), GET( "Ident(1)" ), GET( "Class(1)" ),
      SEE( "UseDefs" ), SET( "UseDefs=0" ), SEE( "UseDefs" ), GET( "ID" ),
      SET( "Epoch=2001" ), GET( "Epoch" ), SET( "MatchEnd=1" ),
      GET( "MatchEnd" ), GET( "MinAxes" ), GET( "NormUnit(1)" ), GET( " " ),
      TEST( "" ), SET( "Label=both" ), GET( "Label(1)" ), GET( "Label(2)" ),
      TEST( "Label" ), GET( "Label" ), CLEAR( "Label" ), GET( "Label(1)" ),
      SET( "Fred=1" ), GET( "Fred" ), TEST( "Fred" ), CLEAR( "Fred" ),
      SET( "Label(2)=two words" ), GET( "Label(2)" ),
   } },
   { "cmp_dump", "cmp_spec_sky", {
      SET( "System(1)=FREQ,System(2)=Galactic,Label(2)=lon,Epoch=2005,"
           "RestFreq=115.27,StdOfRest=LSRK,Equinox=J2010,Title=cube,"
           "Domain=CUBE,Digits=6,Digits(3)=9,Ident=cmp,Unit(1)=GHz" ),
   } },

/* FrameSet's own attributes, and the names it hands to its current Frame,
   on a FrameSet of a plain Frame (Domain GRID) and a SkyFrame. */
   { "fs_own", "fs_grid_sky", {
      GET( "Class" ), SEE( "Nframe" ), SEE( "Base" ), SEE( "Current" ),
      SEE( "Nin" ), SEE( "Nout" ), SEE( "Invert" ), SEE( "Report" ),
      SEE( "TranForward" ), SEE( "TranInverse" ), SEE( "AllVariants" ),
      SEE( "Variant" ), SET( "Base=2" ), SEE( "Base" ), SET( "Current=Base" ),
      SEE( "Current" ), SET( "Current=SKY" ), SEE( "Current" ),
      SET( "Current=1" ), GET( "Current" ), SET( "Current=sky " ),
      GET( "Current" ), SET( "Current=9" ), SET( "Current=0" ),
      SET( "Current=nonsense" ), SET( "Base=nonsense" ), SET( "Base=grid" ),
      GET( "Base" ), SET( "Base=Current" ), GET( "Base" ),
      SET( "Base=AST__BASE" ), SET( "Current=AST__CURRENT" ), GET( "Current" ),
      CLEAR( "Base" ), SEE( "Base" ), CLEAR( "Current" ), SEE( "Current" ),
      SET( "Nframe=3" ), CLEAR( "Nframe" ), SET( "AllVariants=x" ),
      CLEAR( "Class" ), SET( "Nin=1" ), CLEAR( "TranForward" ),
      CYCLE( "Report", "Report=1" ), CYCLE( "ID", "ID=fs id" ),
      CYCLE( "Ident", "Ident=fs" ), SEE( "UseDefs" ), SET( "UseDefs=0" ),
      SEE( "UseDefs" ), CLEAR( "UseDefs" ), SEE( "UseDefs" ),
      GET( "IsLinear" ), GET( "IsSimple" ), TEST( "IsLinear" ),
      SET( "Fred=1" ), GET( "Fred" ), TEST( "Fred" ), CLEAR( "Fred" ),
      GET( "Title" ), GET( "Domain" ), GET( "Naxes" ), GET( "Label(1)" ),
      SET( "Label(1)=lon" ), SEE( "Label(1)" ), GET( " " ),
   } },
   { "fs_integrity", "fs_grid_sky", {
      SET( "System=FK4" ), SEE( "System" ), GET( "Equinox" ),
      SET( "Epoch=1950" ), SEE( "Epoch" ), SET( "Equinox=J2000,System=FK5" ),
      GET( "System" ), CLEAR( "System,Epoch" ), SEE( "System" ),
      SEE( "Epoch" ), SET( "SkyRef=1:00:00,10:00:00,SkyRefIs=Origin" ),
      GET( "SkyRef" ), GET( "SkyRefIs" ),
   } },
   { "fs_failed", "fs_grid_sky", {
      SET( "System=FK4,Fred=1,Grid=1" ), SEE( "System" ),
   } },
   { "fs_failed_clear", "fs_grid_sky", {
      SET( "System=Galactic" ), CLEAR( "System,Fred,Epoch" ), SEE( "System" ),
   } },
   { "fs_invert", "fs_grid_sky", {
      SET( "Invert=1" ), SEE( "Invert" ), GET( "Current" ), GET( "Base" ),
      GET( "Nin" ), GET( "Nout" ), GET( "System" ), GET( "Domain" ),
      SET( "Current=1" ), GET( "System" ), GET( "Base" ),
      SET( "Invert=0" ), GET( "Current" ), CLEAR( "Invert" ), SEE( "Invert" ),
      SET( "Invert=1,System=Cartesian,Invert=0" ), GET( "System" ),
   } },
   { "fs_lost", "fs_grid_sky", {
      SET( "System=Unknown" ), GET( "System" ), SET( "Domain=OTHER" ),
      GET( "Domain" ), SET( "AlignSystem=Galactic" ), GET( "AlignSystem" ),
   } },
   { "fs_aitoff", "fs_aitoff", {
      GET( "Class" ), GET( "Nframe" ), GET( "Current" ), GET( "Base" ),
      GET( "Domain" ), GET( "System" ), SET( "System=ICRS" ), GET( "System" ),
      SET( "Current=GRID" ), GET( "Current" ), GET( "Domain" ),
      SET( "Label(1)=X" ), GET( "Label(1)" ), SET( "Current=SKY" ),
      GET( "Label(1)" ), SET( "Grid=1" ), SET( "System=Galactic" ),
   } },
   { "fs_timeplot", "fs_timeplot", {
      GET( "Nframe" ), GET( "Current" ), GET( "Class" ), GET( "Naxes" ),
      GET( "Domain" ), GET( "Title" ), GET( "System(1)" ), GET( "System(2)" ),
      SET( "Title=a time plot" ), GET( "Title" ),
   } },

/* Plot's own attributes, on a Plot of aitoff.head as testgrid.c makes
   one. */
   { "plot_own", "plot_aitoff", {
      GET( "Class" ), SEE( "Tol" ), CYCLE( "Tol", "Tol=0.05" ), SET( "Tol=2" ),
      GET( "Tol" ), SET( "Tol=x" ), SEE( "Grid" ), CYCLE( "Grid", "Grid=1" ),
      SET( "Grid=7" ), GET( "Grid" ), CLEAR( "Grid" ),
      SEE( "TickAll" ), CYCLE( "TickAll", "TickAll=0" ),
      SEE( "ForceExterior" ), CYCLE( "ForceExterior", "ForceExterior=1" ),
      SEE( "Invisible" ), CYCLE( "Invisible", "Invisible=1" ),
      SEE( "Border" ), CYCLE( "Border", "Border=1" ),
      SEE( "ClipOp" ), CYCLE( "ClipOp", "ClipOp=1" ),
      SEE( "Clip" ), CYCLE( "Clip", "Clip=1" ), SET( "Clip=5" ), GET( "Clip" ),
      CLEAR( "Clip" ), SEE( "Grf" ), CYCLE( "Grf", "Grf=1" ), SET( "Grf=3" ),
      GET( "Grf" ), CLEAR( "Grf" ),
      SEE( "DrawTitle" ), CYCLE( "DrawTitle", "DrawTitle=0" ),
      SEE( "Escape" ), CYCLE( "Escape", "Escape=0" ),
      SEE( "TitleGap" ), CYCLE( "TitleGap", "TitleGap=0.1" ),
      SEE( "TextGapType" ), SET( "TextGapType=plot" ), SEE( "TextGapType" ),
      SET( "TextGapType=b" ), GET( "TextGapType" ), SET( "TextGapType=bad" ),
      SET( "TextGapType= Plot " ), GET( "TextGapType" ), CLEAR( "TextGapType" ),
      SEE( "TextGapType" ), SET( "Labelling=int" ), SEE( "Labelling" ),
      SET( "Labelling=e" ), GET( "Labelling" ), SET( "Labelling=sideways" ),
      SET( "Labelling=exteriors" ), CLEAR( "Labelling" ), TEST( "Labelling" ),
   } },
   { "plot_axis", "plot_aitoff", {
      SEE( "DrawAxes" ), SEE( "DrawAxes(1)" ), SET( "DrawAxes=0" ),
      SEE( "DrawAxes(2)" ), SET( "DrawAxes(2)=1" ), GET( "DrawAxes(2)" ),
      SET( "DrawAxes(3)=1" ), GET( "DrawAxes(3)" ), TEST( "DrawAxes(0)" ),
      CLEAR( "DrawAxes(3)" ), CLEAR( "DrawAxes" ), SEE( "DrawAxes(1)" ),
      SEE( "DrawAxes(2)" ), TEST( "DrawAxes" ),
      SEE( "Abbrev(1)" ), CYCLE( "Abbrev(2)", "Abbrev(2)=0" ), SET( "Abbrev=0" ),
      SEE( "Abbrev" ),
      SET( "Gap=10" ), SEE( "Gap" ), SEE( "Gap(1)" ), SEE( "Gap(2)" ),
      SET( "Gap(2)=5" ), GET( "Gap" ), GET( "Gap(2)" ), CLEAR( "Gap" ),
      TEST( "Gap(1)" ), TEST( "Gap(2)" ), SET( "Gap(4)=1" ), GET( "Gap(4)" ),
      SET( "LogGap=100" ), SEE( "LogGap(2)" ), CLEAR( "LogGap(2)" ),
      SEE( "LogGap(2)" ), SEE( "NumLabGap" ), SET( "NumLabGap(1)=0.1" ),
      SEE( "NumLabGap(1)" ), SEE( "TextLabGap(2)" ),
      SET( "TextLabGap=0.03" ), SEE( "TextLabGap(2)" ), SEE( "LabelUp" ),
      SET( "LabelUp(2)=1" ), SEE( "LabelUp(2)" ), SEE( "NumLab(1)" ),
      CYCLE( "NumLab(1)", "NumLab(1)=0" ), SET( "MinTick=0" ),
      SEE( "MinTick(1)" ), SET( "MinTick(2)=4" ), SEE( "MinTick(2)" ),
      SET( "TextLab(1)=0" ), SEE( "TextLab(1)" ), SET( "LabelUnits=1" ),
      SEE( "LabelUnits(2)" ), SET( "LogTicks(1)=1" ), SEE( "LogTicks(1)" ),
      SET( "LogLabel=1" ), SEE( "LogLabel(2)" ), SET( "MajTickLen=0.02" ),
      SEE( "MajTickLen(1)" ), SET( "MinTickLen(2)=0.01" ),
      SEE( "MinTickLen(2)" ), SEE( "MinTickLen" ),
      SET( "Edge(1)=t" ), SEE( "Edge(1)" ), SET( "Edge(2)=bottom" ),
      GET( "Edge(2)" ), SET( "Edge(1)=x" ), SET( "Edge(1)=" ),
      SET( "Edge=left" ), SET( "Edge(3)=left" ), CLEAR( "Edge(1)" ),
      TEST( "Edge(1)" ), SET( "LabelAt(1)=0.5" ), SEE( "LabelAt(1)" ),
      SET( "LabelAt=0.5" ), SET( "Centre(2)=0.1" ), SEE( "Centre(2)" ),
      CLEAR( "Centre(2)" ), TEST( "Centre(2)" ), SET( "Gap(1)=x" ),
      SET( "Gap=x" ),
   } },
   { "plot_logplot", "plot_frame", {
      SEE( "LogPlot" ), SET( "LogPlot(1)=1" ), SEE( "LogPlot(1)" ),
      SET( "LogPlot=1" ), SEE( "LogPlot(2)" ), CLEAR( "LogPlot" ),
      SEE( "LogPlot(1)" ),
   } },
   { "plot_style", "plot_aitoff", {
      SEE( "Colour(axes)" ), SET( "Colour(axes)=2" ), SEE( "Colour(axes)" ),
      SEE( "Colour(axis1)" ), SEE( "Colour(axis2)" ), SEE( "Colour(axis3)" ),
      SET( "Colour=3" ), SEE( "Colour(border)" ), SEE( "Colour" ),
      SET( "Color(Grid1)=4" ), SEE( "Color(Grid1)" ), SEE( "Colour(grid)" ),
      SEE( "Color" ), SET( "Style(nosuch)=1" ), GET( "Style(nosuch)" ),
      TEST( "Style(nosuch)" ), CLEAR( "Style(nosuch)" ),
      SET( "Style(abcdefghijklmnopqrstu)=1" ), GET( "Style(abcdefghijklmnopqrstu)" ),
      SET( "Style(ax)=2" ), SET( "Style(t)=2" ), SET( "Style(ti)=2" ),
      SEE( "Style(title)" ), SET( "Style(grid)=3" ), SEE( "Style(grid2)" ),
      SEE( "Style" ), SET( "Width(border)=2.5" ), SEE( "Width(border)" ),
      SEE( "Width" ), SET( "Width(curves)=0" ), SET( "Width=0" ),
      SET( "Width(axes)=3" ), GET( "Width(axis2)" ), SET( "Size=1.5" ),
      SEE( "Size(title)" ), SEE( "Size" ), SET( "Size(strings)=0" ),
      SET( "Font(title)=3" ), SEE( "Font(title)" ), SEE( "Font" ),
      SET( "Font(textlab)=2" ), SEE( "Font" ), SEE( "Font(textlab)" ),
      CLEAR( "Colour(axes)" ), SEE( "Colour(axes)" ), SEE( "Colour(axis1)" ),
      TEST( "Colour(numlab)" ), CLEAR( "Style" ), SEE( "Style(grid1)" ),
      CLEAR( "Color(Grid)" ), SET( "Style( Axes )=2" ), SET( "Colour(axes)=x" ),
      SET( "Colour()=1" ), GET( "Colour()" ),
   } },
   { "plot_frameset", "plot_aitoff", {
      SET( "Title=Logarithmic axis example" ), SEE( "Title" ),
      SET( "Label(1)=First axis" ), SEE( "Label(1)" ), SET( "System=ICRS" ),
      GET( "System" ), GET( "Nframe" ), GET( "Current" ), GET( "Base" ),
      GET( "Domain" ), SET( "Current=1" ), GET( "Domain" ), SET( "Fred=1" ),
      GET( "Fred" ), TEST( "Fred" ), CLEAR( "Fred" ), SET( "Current=SKY" ),
      SET( "Fred=1" ), SET( "Grid=1,Fred=2" ), GET( "Grid" ),
      SET( "Ident=plot" ), GET( "Ident" ), SEE( "Invert" ),
      GET( "Nin" ), GET( "Naxes" ), GET( "Epoch" ),
   } },
   { "plot_used", "plot_aitoff", {
      GET( "Grid" ), GET( "Border" ), GET( "Labelling" ), GET( "Edge(1)" ),
      GET( "Edge(2)" ), GET( "LabelAt(1)" ), GET( "LabelAt(2)" ),
      GET( "Centre(1)" ), GET( "Centre(2)" ), GET( "Gap" ), GET( "Gap(1)" ),
      GET( "Gap(2)" ), GET( "LogGap(1)" ), GET( "LogGap" ), GET( "MinTick(1)" ),
      GET( "MinTick" ), GET( "TextLab(1)" ), GET( "TextLab" ),
      GET( "LabelUnits(1)" ), GET( "LabelUnits" ), GET( "LogTicks(2)" ),
      GET( "LogLabel(1)" ), GET( "MajTickLen(1)" ), GET( "MajTickLen" ),
      SET( "Edge(2)=right" ), GET( "Edge(2)" ), GET( "Edge(1)" ),
      GET( "LabelAt(3)" ), GET( "Centre(0)" ), GET( "Edge(3)" ),
   } },
   { "plot_used_frame", "plot_frame", {
      GET( "Grid" ), GET( "Labelling" ), GET( "Edge(1)" ), GET( "Edge(2)" ),
      GET( "Gap(1)" ), GET( "Gap(2)" ), GET( "Centre(1)" ), GET( "MinTick(2)" ),
      SET( "Gap(1)=2.5" ), GET( "Gap(1)" ), GET( "Centre(1)" ),
   } },
   { "plot_gap0", "plot_frame", {
      SET( "Gap=0" ), GET( "Gap(1)" ), GET( "Grid" ), GET( "Edge(1)" ),
      GET( "Labelling" ),
   } },
   { "plot_cgi_aitoff", "plot_aitoff", {
      SET( "System=ICRS, Grid=1, Tickall=0, Border=1" ),
   } },
   { "plot_cgi_cobe", "plot_aitoff", {
      SET( "Format(1)=gd, Format(2)=gd, Grid=1, Tickall=0, Width(axes)=3, Colour(axes)=2" ),
   } },
   { "plot_cgi_origin", "plot_aitoff", {
      SET( "Format(1)=ghms, Format(2)=gdm, Edge(2)=r, Tickall=0" ),
   } },
   { "plot_cgi_scp", "plot_aitoff", {
      SET( "Grid=1, Labelling=int" ),
   } },
   { "plot_cgi_spec2d", "plot_aitoff", {
      SET( "System(1)=wave, LogTicks(1)=1, LabelUnits=1, MinTickLen=0.02, "
           "MajTickLen=0.05, NumLabGap=0.1, Title=Logarithmic axis example" ),
   } },
   { "plot_cgi_spec2d_rest", "plot_aitoff", {
      SET( "LogTicks(1)=1, LabelUnits=1, MinTickLen=0.02, "
           "MajTickLen=0.05, NumLabGap=0.1, Title=Logarithmic axis example" ),
   } },

/* A FitsChan holding a header's cards, positioned at the first: FitsChan's
   and Channel's attributes. */
   { "fc_aitoff", "fc_aitoff", {
      GET( "Class" ), SEE( "Encoding" ), SET( "Encoding=FITS-WCS" ),
      SEE( "Encoding" ), SET( "Encoding=fits_iraf" ), GET( "Encoding" ),
      SET( "Encoding=native" ), GET( "Encoding" ), SET( "Encoding=bad" ),
      SET( "Encoding=FITS-AIPS++" ), GET( "Encoding" ), SET( "Encoding=dss" ),
      GET( "Encoding" ), SET( "Encoding=FITS-CLASS" ), GET( "Encoding" ),
      SET( "Encoding=FITS-PC" ), GET( "Encoding" ), SET( "Encoding=FITS-AIPS" ),
      GET( "Encoding" ), CLEAR( "Encoding" ), SEE( "Encoding" ),
      SEE( "Card" ), SET( "Card=3" ), SEE( "Card" ), SEE( "CardName" ),
      SEE( "CardType" ), SEE( "CardComm" ), SEE( "NCard" ), SEE( "NKey" ),
      SET( "Card=100" ), GET( "Card" ), GET( "CardName" ), GET( "CardType" ),
      GET( "CardComm" ), CLEAR( "Card" ), SEE( "Card" ), GET( "CardName" ),
      SEE( "AllWarnings" ), SEE( "Warnings" ), SET( "Warnings=Tnx Zpx" ),
      SEE( "Warnings" ), SET( "Warnings=nonsense" ), CLEAR( "Warnings" ),
      SEE( "Warnings" ), SEE( "AltAxes" ), SET( "AltAxes=bad" ),
      SET( "AltAxes=ident" ), SEE( "AltAxes" ), SET( "AltAxes=n" ),
      GET( "AltAxes" ), CLEAR( "AltAxes" ), SEE( "AltAxes" ),
      CLEAR( "NCard" ), CLEAR( "AllWarnings" ), CLEAR( "CardName" ),
      CLEAR( "CardType" ), CLEAR( "CardComm" ), CLEAR( "NKey" ),
      SET( "NCard=3" ), SET( "CardName=x" ), SET( "AllWarnings=x" ),
      SEE( "CarLin" ), CYCLE( "CarLin", "CarLin=1" ),
      SEE( "FitsDigits" ), CYCLE( "FitsDigits", "FitsDigits=8" ),
      SET( "FitsDigits=-4" ), GET( "FitsDigits" ),
      SEE( "FitsRounding" ), CYCLE( "FitsRounding", "FitsRounding=5" ),
      SEE( "CDMatrix" ), CYCLE( "CDMatrix", "CDMatrix=1" ),
      SEE( "DefB1950" ), CYCLE( "DefB1950", "DefB1950=0" ),
      SEE( "TabOK" ), CYCLE( "TabOK", "TabOK=2" ),
      SEE( "ForceTab" ), CYCLE( "ForceTab", "ForceTab=1" ),
      SEE( "IgnoreBadAlt" ), CYCLE( "IgnoreBadAlt", "IgnoreBadAlt=1" ),
      SEE( "SipReplace" ), CYCLE( "SipReplace", "SipReplace=0" ),
      SEE( "FitsTol" ), CYCLE( "FitsTol", "FitsTol=0.5" ),
      SEE( "PolyTan" ), CYCLE( "PolyTan", "PolyTan=1" ),
      SEE( "SipOK" ), CYCLE( "SipOK", "SipOK=0" ),
      SEE( "Iwc" ), CYCLE( "Iwc", "Iwc=1" ),
      SEE( "Clean" ), CYCLE( "Clean", "Clean=1" ),
      SEE( "FitsAxisOrder" ), CYCLE( "FitsAxisOrder", "FitsAxisOrder=<copy>" ),
      SEE( "Comment" ), CYCLE( "Comment", "Comment=0" ),
      SEE( "Full" ), CYCLE( "Full", "Full=1" ), SEE( "Indent" ),
      CYCLE( "Indent", "Indent=2" ), SEE( "ReportLevel" ),
      CYCLE( "ReportLevel", "ReportLevel=2" ), SEE( "Skip" ),
      CYCLE( "Skip", "Skip=1" ), SEE( "Strict" ), CYCLE( "Strict", "Strict=1" ),
      SEE( "SinkFile" ), SEE( "SourceFile" ),
      CYCLE( "ID", "ID=fc" ), CYCLE( "Ident", "Ident=fits" ),
      CYCLE( "UseDefs", "UseDefs=0" ), SEE( "Class" ), SET( "Class=x" ),
      SET( "Fred=1" ), GET( "Fred" ), TEST( "Fred" ), CLEAR( "Fred" ),
      GET( " " ), SET( "CarLin=1 x" ), SET( "Encoding=FITS-WCS,FitsDigits=8" ),
      GET( "Encoding" ), GET( "FitsDigits" ),
   } },
   { "fc_solar", "fc_solar", {
      SEE( "NCard" ), SEE( "NKey" ), GET( "Encoding" ), GET( "Card" ),
      SET( "Card=6" ), GET( "CardName" ), GET( "CardType" ), GET( "CardComm" ),
      SET( "Card=7" ), GET( "CardName" ), GET( "CardType" ), GET( "CardComm" ),
      SET( "Card=8" ), GET( "CardName" ), GET( "CardType" ), GET( "CardComm" ),
      SET( "Card=14" ), GET( "CardName" ), GET( "CardComm" ),
   } },
};

/* The header of the CGI page's SOLAR example. */
static const char *solar_head =
"SIMPLE  =  T         /Simple FITS file\n"
"BITPIX  =  -32       /Real*4 (Floating point)\n"
"NAXIS   =  2         /Two-dimensional image\n"
"NAXIS1  =  1024      /Number of pixels per column\n"
"NAXIS2  =  1024      /Number of pixels per row\n"
"\n"
"COMMENT Heliocentric-Cartesian coordinates\n"
"\n"
"CTYPE1  = 'SOLX    ' /Coordinate axis labels\n"
"CTYPE2  = 'SOLY    ' /\n"
"CRPIX1  = 512.5      /Reference pixel is center of CCD\n"
"CRPIX2  =  512.5     /\n"
"CUNIT1  = 'solRad  ' /Coordinates are in solar radii\n"
"CUNIT2  = 'solRad  ' /\n"
"CD1_1   = 0.00375    /Plate scale in solar radii per pixel\n"
"CD2_2   = 0.00375    /\n"
"CRVAL1  = 0.0        /Reference pixel is Sun center\n"
"CRVAL2  = 0.0        /\n"
"END";

/* The header text a FitsChan case starts from, held whole. */
static char headbuf[ 65536 ];

/* The text of a header for a FitsChan case. */
static const char *head_text( const char *id ) {
   FILE *fp;
   size_t n;
   if( !strcmp( id, "fc_solar" ) ) return solar_head;
   fp = fopen( HEADS "aitoff.head", "r" );
   if( !fp ) { fprintf( stderr, "cannot open aitoff.head\n" ); exit( 1 ); }
   n = fread( headbuf, 1, sizeof( headbuf ) - 1, fp );
   fclose( fp );
   while( n > 0 && headbuf[ n - 1 ] == '\n' ) n--;
   headbuf[ n ] = '\0';
   return headbuf;
}

/* A FitsChan holding the lines of a header text, at its first card. */
static AstFitsChan *make_fitschan( const char *text ) {
   char line[ 256 ];
   const char *p = text;
   AstFitsChan *fc = astFitsChan( NULL, NULL, " " );
   while( *p ) {
      size_t n = strcspn( p, "\n" );
      if( n >= sizeof( line ) ) n = sizeof( line ) - 1;
      memcpy( line, p, n );
      line[ n ] = '\0';
      astPutFits( fc, line, 0 );
      p += n;
      if( *p == '\n' ) p++;
   }
   astClear( fc, "Card" );
   return fc;
}

/* The object a case starts from. */
static AstObject *make( const char *id ) {
   if( !strncmp( id, "fc_", 3 ) ) return (AstObject *) make_fitschan( head_text( id ) );
   if( !strcmp( id, "frame2" ) ) return (AstObject *) astFrame( 2, " " );
   if( !strcmp( id, "frame1" ) ) return (AstObject *) astFrame( 1, " " );
   if( !strcmp( id, "sky" ) ) return (AstObject *) astSkyFrame( " " );
   if( !strcmp( id, "sky_perm" ) ) {
      int perm[ 2 ] = { 2, 1 };
      AstSkyFrame *sky = astSkyFrame( " " );
      astPermAxes( sky, perm );
      return (AstObject *) sky;
   }
   if( !strcmp( id, "spec" ) ) return (AstObject *) astSpecFrame( " " );
   if( !strcmp( id, "dsb" ) ) return (AstObject *) astDSBSpecFrame( " " );
   if( !strcmp( id, "time" ) ) return (AstObject *) astTimeFrame( " " );
   if( !strcmp( id, "cmp_spec_sky" ) ) {
      return (AstObject *) astCmpFrame( astSpecFrame( " " ), astSkyFrame( " " ), " " );
   }
   if( !strcmp( id, "cmp_1_1" ) ) {
      return (AstObject *) astCmpFrame( astFrame( 1, " " ), astFrame( 1, " " ), " " );
   }
   if( !strcmp( id, "fs_grid_sky" ) ) {
      AstFrameSet *fs = astFrameSet( astFrame( 2, "Domain=GRID" ), " " );
      astAddFrame( fs, 1, astUnitMap( 2, " " ), astSkyFrame( " " ) );
      return (AstObject *) fs;
   }
   if( !strcmp( id, "fs_aitoff" ) ) return (AstObject *) read_head( HEADS "aitoff.head", "" );
   if( !strcmp( id, "fs_timeplot" ) ) return (AstObject *) read_head( HEADS "timeplot.head", "" );
   if( !strcmp( id, "plot_aitoff" ) ) {
      return (AstObject *) testgrid_plot( "aitoff.head", "", NULL, "" );
   }
   if( !strcmp( id, "plot_frame" ) ) {
      float gbox[ 4 ] = { 0.1f, 0.1f, 0.9f, 0.9f };
      double pbox[ 4 ] = { 1.0, 1.0, 10.0, 10.0 };
      AstFrame *f = astFrame( 2, "Domain=DATA" );
      AstPlot *pl = astPlot( f, gbox, pbox, " " );
      f = astAnnul( f );
      return (AstObject *) pl;
   }
   if( !strcmp( id, "skyaxis1" ) ) {
      int axes[ 1 ] = { 1 };
      AstSkyFrame *sky = astSkyFrame( " " );
      AstObject *obj = (AstObject *) astPickAxes( sky, 1, axes, NULL );
      sky = astAnnul( sky );
      return obj;
   }
   fprintf( stderr, "unknown object %s\n", id );
   exit( 1 );
}

/* The Rust name of an error status. */
static const char *code_name( int status ) {
   switch( status ) {
      case AST__BADAT: return "Badat";
      case AST__NOWRT: return "Nowrt";
      case AST__ATTIN: return "Attin";
      case AST__AXIIN: return "Axiin";
      case AST__OPT: return "Opt";
      case AST__BADOC: return "Badoc";
      case AST__BADUN: return "Badun";
      case AST__INTER: return "Inter";
      case AST__SCSIN: return "Scsin";
      case AST__DTERR: return "Dterr";
      case AST__ILOST: return "Ilost";
      case AST__PLFMT: return "Plfmt";
      case AST__GRFER: return "Grfer";
      case AST__ATGER: return "Atger";
      case AST__ATSER: return "Atser";
      case AST__BDFTS: return "Bdfts";
      case AST__UNFER: return "Unfer";
      case AST__BADIN: return "Badin";
      case AST__OBJIN: return "Objin";
      case AST__FRMIN: return "Frmin";
      case AST__INNCO: return "Innco";
      case AST__NOFTS: return "Nofts";
      default:
         fprintf( stderr, "status %d has no name in code_name\n", status );
         exit( 1 );
   }
}

/* A Rust string literal, with control characters escaped. */
static void rs( const char *s ) {
   putchar( '"' );
   for( ; *s; s++ ) {
      unsigned char c = (unsigned char) *s;
      if( c == '"' || c == '\\' ) {
         putchar( '\\' );
         putchar( c );
      } else if( c == '\n' ) {
         fputs( "\\n", stdout );
      } else if( c == '\r' ) {
         fputs( "\\r", stdout );
      } else if( c == '\t' ) {
         fputs( "\\t", stdout );
      } else if( c < 0x20 || c == 0x7f ) {
         printf( "\\x%02x", c );
      } else {
         putchar( c );
      }
   }
   putchar( '"' );
}

static char dumpbuf[ 1 << 20 ];
static void sink( const char *line ) {
   strncat( dumpbuf, line, sizeof( dumpbuf ) - strlen( dumpbuf ) - 2 );
   strcat( dumpbuf, "\n" );
}

/* Print the Native dump of an object as a Rust raw string. */
static void dump( AstObject *obj ) {
   AstChannel *ch = astChannel( NULL, sink, " " );
   dumpbuf[ 0 ] = '\0';
   astWrite( ch, obj );
   ch = astAnnul( ch );
   printf( "r###\"%s\"###", dumpbuf );
}

/* Print the outcome of the operation just applied: the error if there was
   one, and otherwise what "ok" prints. */
static int failed( void ) {
   int i;
   if( astOK ) {
      if( nmsg ) {
         fprintf( stderr, "messages without an error status\n" );
         exit( 1 );
      }
      return 0;
   }
   printf( "Out::Err(E::%s, &[", code_name( astStatus ) );
   for( i = 0; i < nmsg; i++ ) {
      if( i ) printf( ", " );
      rs( msgs[ i ] );
   }
   printf( "])" );
   astClearStatus;
   nmsg = 0;
   return 1;
}

int main( void ) {
   int status = 0;
   size_t c;
   astWatch( &status );
   astGrfLogInit( NULL );

   printf( "// Generated by tools/oracle_fixtures/attr_strings_probe.c against\n"
           "// the C library at the submodule pin. Do not edit.\n\n"
           "use starlink_ast::error::ErrorCode as E;\n\n"
           "/// One operation on a case's object.\n"
           "#[derive(Debug, Clone, Copy, PartialEq)]\n"
           "pub enum Op {\n"
           "    /// `astSet( obj, \"%%s\", settings )`.\n"
           "    Set(&'static str),\n"
           "    /// `astSetC( obj, name, value )`.\n"
           "    SetC(&'static str, &'static str),\n"
           "    /// `astGetC( obj, name )`.\n"
           "    Get(&'static str),\n"
           "    /// `astTest( obj, name )`.\n"
           "    Test(&'static str),\n"
           "    /// `astClear( obj, names )`.\n"
           "    Clear(&'static str),\n"
           "}\n\n"
           "/// What the C library did with an operation: an error is its\n"
           "/// status and every message reported, in order.\n"
           "#[derive(Debug, Clone, PartialEq)]\n"
           "pub enum Out {\n"
           "    Done,\n"
           "    Value(&'static str),\n"
           "    Flag(bool),\n"
           "    Err(E, &'static [&'static str]),\n"
           "}\n\n"
           "/// A case: the object as Native text (for a FitsChan, its header\n"
           "/// cards), the operations with C's outcomes, and the Native dump\n"
           "/// afterwards (empty for a FitsChan).\n"
           "pub struct Case {\n"
           "    pub name: &'static str,\n"
           "    pub fitschan: bool,\n"
           "    pub object: &'static str,\n"
           "    pub ops: &'static [(Op, Out)],\n"
           "    pub dump: &'static str,\n"
           "}\n\n"
           "pub const CASES: &[Case] = &[\n" );

   for( c = 0; c < sizeof( cases ) / sizeof( cases[ 0 ] ); c++ ) {
      const struct acase *ac = &cases[ c ];
      const struct op *op;
      AstObject *obj = make( ac->object );
      if( !astOK ) {
         fprintf( stderr, "case %s: cannot make the object\n", ac->name );
         return 1;
      }
      int fitschan = !strncmp( ac->object, "fc_", 3 );
      printf( "    Case {\n        name: " );
      rs( ac->name );
      printf( ",\n        fitschan: %s,\n        object: ", fitschan ? "true" : "false" );
      if( fitschan ) {
         printf( "r###\"%s\"###", head_text( ac->object ) );
      } else {
         dump( obj );
      }
      printf( ",\n        ops: &[\n" );
      for( op = ac->ops; op->kind != OP_END; op++ ) {
         const char *value;
         int flag;
         printf( "            (" );
         fflush( stdout );
         switch( op->kind ) {
            case OP_SET:
               printf( "Op::Set(" ); rs( op->a ); printf( "), " );
               astSet( obj, "%s", op->a );
               if( !failed() ) printf( "Out::Done" );
               break;
            case OP_SETC:
               printf( "Op::SetC(" ); rs( op->a ); printf( ", " );
               rs( op->b ); printf( "), " );
               astSetC( obj, op->a, op->b );
               if( !failed() ) printf( "Out::Done" );
               break;
            case OP_GET:
               printf( "Op::Get(" ); rs( op->a ); printf( "), " );
               value = astGetC( obj, op->a );
               if( !failed() ) {
                  printf( "Out::Value(" ); rs( value ); printf( ")" );
               }
               break;
            case OP_TEST:
               printf( "Op::Test(" ); rs( op->a ); printf( "), " );
               flag = astTest( obj, op->a );
               if( !failed() ) printf( "Out::Flag(%s)", flag ? "true" : "false" );
               break;
            default:
               printf( "Op::Clear(" ); rs( op->a ); printf( "), " );
               astClear( obj, op->a );
               if( !failed() ) printf( "Out::Done" );
               break;
         }
         printf( "),\n" );
      }
      printf( "        ],\n        dump: " );
      if( fitschan ) {
         printf( "\"\"" );
      } else {
         dump( obj );
      }
      printf( ",\n    },\n" );
      obj = astAnnul( obj );
   }
   printf( "];\n" );
   return astOK ? 0 : 1;
}
