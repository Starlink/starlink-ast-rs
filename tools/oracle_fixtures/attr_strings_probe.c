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

#define MAXOP 160
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
};

/* The object a case starts from. */
static AstObject *make( const char *id ) {
   if( !strcmp( id, "frame2" ) ) return (AstObject *) astFrame( 2, " " );
   if( !strcmp( id, "frame1" ) ) return (AstObject *) astFrame( 1, " " );
   if( !strcmp( id, "sky" ) ) return (AstObject *) astSkyFrame( " " );
   if( !strcmp( id, "sky_perm" ) ) {
      int perm[ 2 ] = { 2, 1 };
      AstSkyFrame *sky = astSkyFrame( " " );
      astPermAxes( sky, perm );
      return (AstObject *) sky;
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
      printf( "    Case {\n        name: " );
      rs( ac->name );
      printf( ",\n        fitschan: false,\n        object: " );
      dump( obj );
      printf( ",\n        ops: &[\n" );
      for( op = ac->ops; op->kind != OP_END; op++ ) {
         const char *value;
         int flag;
         printf( "            (" );
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
      dump( obj );
      printf( ",\n    },\n" );
      obj = astAnnul( obj );
   }
   printf( "];\n" );
   return astOK ? 0 : 1;
}
