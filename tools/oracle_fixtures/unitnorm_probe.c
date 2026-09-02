/* unitnorm_probe -- read a native-text Mapping from a file, run one point
 * through its inverse transform, and print the result.
 *
 * Answers "what does the C library on THIS machine compute here?" for oracle
 * rows whose recorded values depend on the generator host's floating point.
 *
 * Usage: ./unitnorm_probe <fixture.map> <x1> <x2> ... <xN>
 *   with N = the Mapping's Nout (the inverse transform's inputs).
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"

static char *text = NULL;
static size_t pos = 0;

/* Hand the channel one line per call, NULL at end of input. */
static const char *source(void) {
    static char line[4096];
    if (!text || text[pos] == '\0') return NULL;
    size_t n = 0;
    while (text[pos] != '\0' && text[pos] != '\n' && n < sizeof(line) - 1) {
        line[n++] = text[pos++];
    }
    if (text[pos] == '\n') pos++;
    line[n] = '\0';
    return line;
}

int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "usage: unitnorm_probe <fixture.map> <x1> ... <xN>\n");
        return 2;
    }

    FILE *f = fopen(argv[1], "r");
    if (!f) { perror(argv[1]); return 1; }
    fseek(f, 0, SEEK_END);
    long len = ftell(f);
    fseek(f, 0, SEEK_SET);
    text = malloc((size_t) len + 1);
    if (fread(text, 1, (size_t) len, f) != (size_t) len) { perror("fread"); return 1; }
    text[len] = '\0';
    fclose(f);

    astBegin;
    AstChannel *chin = astChannel(source, NULL, "%s", "");
    AstMapping *map = (AstMapping *) astRead(chin);
    if (!astOK || !map) { fprintf(stderr, "astRead failed\n"); return 1; }

    int nin = astGetI(map, "Nin");
    int nout = astGetI(map, "Nout");
    int ncoord_in = nout;              /* inverse: the map's outputs feed in */
    if (argc - 2 != ncoord_in) {
        fprintf(stderr, "expected %d coordinates, got %d\n", ncoord_in, argc - 2);
        return 2;
    }

    double *in = malloc(sizeof(double) * (size_t) ncoord_in);
    double *out = malloc(sizeof(double) * (size_t) nin);
    for (int c = 0; c < ncoord_in; c++) in[c] = atof(argv[2 + c]);

    astTranN(map, 1, ncoord_in, 1, in, 0, nin, 1, out);

    for (int c = 0; c < nin; c++) {
        if (out[c] == AST__BAD) printf(" BAD");
        else printf(" %.17g", out[c]);
    }
    printf("\n");

    astEnd;
    return astOK ? 0 : 1;
}
