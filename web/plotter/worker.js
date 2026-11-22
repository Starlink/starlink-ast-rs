// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

// The drawing Worker's code. The page runs it in a Worker made from the
// source of makeWasmBindgen (the wasm-bindgen glue) and of this function;
// tools/plotter_smoke.mjs runs it in Node with a stand-in for the Worker's
// global scope.
//
// The first message carries the compiled module to instantiate, each later
// one a request, and each request gets one reply. A panic in the library
// traps the instance, which is then unusable: the reply reports an internal
// error, and the page replaces the Worker.

// eslint-disable-next-line no-unused-vars
function plotterWorker(scope, makeWasmBindgen) {
  let ready = null;
  scope.onmessage = async (event) => {
    const message = event.data;
    if (message.module !== undefined) {
      const wb = makeWasmBindgen();
      ready = wb({ module_or_path: message.module }).then(() => wb);
      return;
    }
    let reply;
    try {
      const plotter = await ready;
      const out = plotter.plot(...message.args);
      try {
        reply = {
          id: message.id,
          svg: out.svg(),
          svgError: out.svg_error(),
          fits: out.fits(),
          fitsError: out.fits_error(),
          ast: out.ast(),
          astError: out.ast_error(),
        };
      } finally {
        out.free();
      }
    } catch (e) {
      reply = { id: message.id, internal: String(e) };
    }
    scope.postMessage(reply);
  };
}
