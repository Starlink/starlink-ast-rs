/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* Minimal grf/grf3d stubs — probe never plots; satisfy the linker only. */
int astGAttr(int a, double b, double *c, int d){(void)a;(void)b;(void)c;(void)d;return 0;}
int astGBBuf(void){return 0;}
int astGCap(int a,int b){(void)a;(void)b;return 0;}
int astGEBuf(void){return 0;}
int astGLine(int a,const float*b,const float*c){(void)a;(void)b;(void)c;return 0;}
int astGMark(int a,const float*b,const float*c,int d){(void)a;(void)b;(void)c;(void)d;return 0;}
int astGQch(float*a,float*b){(void)a;(void)b;return 0;}
int astGScales(float*a,float*b){(void)a;(void)b;return 0;}
int astGText(const char*a,float b,float c,const char*d,float e,float f){(void)a;(void)b;(void)c;(void)d;(void)e;(void)f;return 0;}
int astGTxExt(const char*a,float b,float c,const char*d,float e,float f,float*g,float*h){(void)a;(void)b;(void)c;(void)d;(void)e;(void)f;(void)g;(void)h;return 0;}
int astG3DAttr(int a,double b,double*c,int d){(void)a;(void)b;(void)c;(void)d;return 0;}
int astG3DCap(int a,int b){(void)a;(void)b;return 0;}
int astG3DFlush(void){return 0;}
int astG3DLine(int a,float*b,float*c,float*d){(void)a;(void)b;(void)c;(void)d;return 0;}
int astG3DMark(int a,float*b,float*c,float*d,int e,float*f){(void)a;(void)b;(void)c;(void)d;(void)e;(void)f;return 0;}
int astG3DQch(float*a){(void)a;return 0;}
int astG3DText(const char*a,float*b,const char*c,float*d,float*e){(void)a;(void)b;(void)c;(void)d;(void)e;return 0;}
int astG3DTxExt(const char*a,float*b,const char*c,float*d,float*e,float*f){(void)a;(void)b;(void)c;(void)d;(void)e;(void)f;return 0;}
#include <stdio.h>
void astPutErr_(int status, const char *message){ (void)status; fprintf(stderr,"%s\n",message); }
