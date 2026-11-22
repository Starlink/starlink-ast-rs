// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

// The page's behaviour: fill the inputs from an example, redraw shortly
// after the last edit, and show each output or the error that replaced it.
// makeWasmBindgen, plotterWorker (worker.js), PLOTTER_EXAMPLES and
// PLOTTER_WASM are defined by the scripts the build puts before this one.
//
// Drawing runs in a Worker, so the page stays responsive: some settings
// (a very small Tol, a very large MinTick) make a grid take minutes, in the
// C library as here. An edit while a draw runs stops its Worker and starts
// again, and a draw that runs past the time limit is stopped and reported.

'use strict';

(function () {
  const REDRAW_DELAY_MS = 400;
  const TIME_LIMIT_MS = 20000;
  const $ = (id) => document.getElementById(id);
  const inputs = ['header', 'plot-settings', 'fitschan-settings', 'width', 'height', 'version'];

  let compiled = null; // a promise of the WebAssembly.Module, compiled once
  let workerUrl = null; // the Blob URL of the Worker's script, made once
  let worker = null; // the Worker that draws, or null
  let inFlight = null; // the draw the Worker is doing: { id, start, timer }
  let nextId = 0;
  let generation = 0; // counts redraws, so a superseded one does nothing
  let timer = null;
  let svgUrl = null;

  function wasmBytes() {
    const text = atob(PLOTTER_WASM);
    const bytes = new Uint8Array(text.length);
    for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i);
    return bytes;
  }

  function startWorker(module) {
    if (workerUrl === null) {
      const source =
        `${makeWasmBindgen.toString()}\n${plotterWorker.toString()}\n` +
        'plotterWorker(self, makeWasmBindgen);\n';
      workerUrl = URL.createObjectURL(new Blob([source], { type: 'text/javascript' }));
    }
    const w = new Worker(workerUrl);
    w.onmessage = onReply;
    w.onerror = (event) => {
      event.preventDefault();
      failDraw(`Internal error: the plotter could not run; this is a bug.\n${event.message}`);
    };
    w.postMessage({ module });
    return w;
  }

  function stopWorker() {
    if (inFlight !== null) clearTimeout(inFlight.timer);
    inFlight = null;
    if (worker !== null) worker.terminate();
    worker = null;
  }

  function showText(el, text, error) {
    el.classList.toggle('error', error !== undefined);
    el.textContent = error !== undefined ? error : text;
  }

  function showGrid(svg, error) {
    const el = $('grid');
    if (svgUrl !== null) {
      URL.revokeObjectURL(svgUrl);
      svgUrl = null;
    }
    el.replaceChildren();
    if (error !== undefined) {
      el.classList.add('error');
      el.textContent = error;
      return;
    }
    el.classList.remove('error');
    svgUrl = URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml' }));
    const img = document.createElement('img');
    img.alt = 'Coordinate grid of the header';
    img.src = svgUrl;
    el.appendChild(img);
  }

  // The same message in place of every output, with a new Worker next time.
  function failDraw(message) {
    stopWorker();
    showGrid(undefined, message);
    showText($('fits'), undefined, message);
    showText($('ast'), undefined, message);
    $('status').textContent = '';
  }

  function onReply(event) {
    const reply = event.data;
    if (inFlight === null || reply.id !== inFlight.id) return;
    const elapsed = performance.now() - inFlight.start;
    clearTimeout(inFlight.timer);
    inFlight = null;
    if (reply.internal !== undefined) {
      failDraw(`Internal error: the plotter stopped on this input; this is a bug.\n${reply.internal}`);
      return;
    }
    showGrid(reply.svg, reply.svgError);
    showText($('fits'), reply.fits, reply.fitsError);
    showText($('ast'), reply.ast, reply.astError);
    $('status').textContent = `Drawn in ${Math.round(elapsed)} ms.`;
  }

  function timedOut() {
    failDraw(
      `Stopped: drawing took longer than ${TIME_LIMIT_MS / 1000} s. ` +
        'Some settings, such as a very small Tol or a very large MinTick, ' +
        'make a grid take minutes.',
    );
  }

  // The size as typed, for the plotter to check: a blank field or text that
  // is not a number is NaN.
  function pixels(id) {
    const text = $(id).value.trim();
    return text === '' ? NaN : Number(text);
  }

  async function redraw() {
    const mine = ++generation;
    // A draw still running is abandoned, so an edit always takes effect
    // however long the previous inputs take to draw.
    if (inFlight !== null) stopWorker();
    $('status').textContent = 'Drawing…';
    let module;
    try {
      if (compiled === null) compiled = WebAssembly.compile(wasmBytes());
      module = await compiled;
    } catch (e) {
      failDraw(`Internal error: the plotter could not be loaded; this is a bug.\n${e}`);
      return;
    }
    if (mine !== generation) return;
    if (worker === null) worker = startWorker(module);
    const id = ++nextId;
    inFlight = { id, start: performance.now(), timer: setTimeout(timedOut, TIME_LIMIT_MS) };
    worker.postMessage({
      id,
      args: [
        $('header').value,
        $('plot-settings').value,
        $('fitschan-settings').value,
        pixels('width'),
        pixels('height'),
        $('version').value,
        true,
      ],
    });
  }

  function schedule() {
    clearTimeout(timer);
    timer = setTimeout(redraw, REDRAW_DELAY_MS);
  }

  // As the CGI page's loadExample does: the header, the Plot settings and
  // the version character come from the example.
  function loadExample(index) {
    const example = PLOTTER_EXAMPLES[index];
    $('header').value = example.header;
    $('plot-settings').value = example.settings;
    $('version').value = example.version.trim();
    clearTimeout(timer);
    redraw();
  }

  const menu = $('example');
  PLOTTER_EXAMPLES.forEach((example, i) => {
    const option = document.createElement('option');
    option.value = String(i);
    option.textContent = example.name;
    menu.appendChild(option);
  });
  menu.addEventListener('change', () => loadExample(Number(menu.value)));
  for (const id of inputs) $(id).addEventListener('input', schedule);

  loadExample(0);
})();
