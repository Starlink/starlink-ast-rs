/* sky_offset_map_probe -- capture the Mapping astSkyOffsetMap returns for
 * SkyFrames with and without an offset coordinate system: its class, and
 * three points transformed forward and inverse.
 * The values are copied into `sky_offset_map_follows_the_c_library` in
 * crates/starlink-ast/tests/test_skyframe.rs.
 */
#include <stdio.h>
#include "ast.h"
static void run(const char *name, const char *attrs, int perm) {
   double in[2][3] = {{1.0, 1.2, 4.0}, {0.5, -0.3, 1.1}};
   double out[2][3];
   AstSkyFrame *sky = astSkyFrame(attrs);
   if (perm) { int p[2] = {2, 1}; astPermAxes(sky, p); }
   AstMapping *m = astSkyOffsetMap(sky);
   if (!astOK) { printf("%s: error %d\n", name, astStatus); astClearStatus; return; }
   printf("%s: class %s", name, astGetC(m, "Class"));
   astTran2(m, 3, in[0], in[1], 1, out[0], out[1]);
   printf(" fwd");
   for (int i=0;i<3;i++) printf(" [%.17g, %.17g]", out[0][i], out[1][i]);
   astTran2(m, 3, in[0], in[1], 0, out[0], out[1]);
   printf(" inv");
   for (int i=0;i<3;i++) printf(" [%.17g, %.17g]", out[0][i], out[1][i]);
   printf("\n");
}
int main(void) {
   run("ignored", "SkyRef(1)=1,SkyRef(2)=0.5,SkyRefIs=Ignored", 0);
   run("unset", "SkyRefIs=Origin", 0);
   run("origin", "SkyRef(1)=1,SkyRef(2)=0.5,SkyRefIs=Origin", 0);
   run("origin_refp", "SkyRef(1)=1,SkyRef(2)=0.5,SkyRefP(1)=0.2,SkyRefP(2)=1.2,SkyRefIs=Origin", 0);
   run("pole", "SkyRef(1)=1,SkyRef(2)=0.5,SkyRefIs=Pole", 0);
   run("origin_lat_only", "SkyRef(2)=0.5,SkyRefIs=Origin", 0);
   run("permuted_origin", "SkyRef(1)=1,SkyRef(2)=0.5,SkyRefIs=Origin", 1);
   run("badoc", "SkyRef(1)=1,SkyRef(2)=0.5,SkyRefP(1)=1,SkyRefP(2)=0.5,SkyRefIs=Origin", 0);
   printf("AST__BADOC=%d\n", AST__BADOC);
   return 0;
}
