// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

// Smoke test of the FITS plotter's WebAssembly build, run by
// tools/build_plotter.sh --smoke.
//
//   node tools/plotter_smoke.mjs <glue.js> <module.wasm> <examples.json> \
//        <worker.js> <date header>
//
// <glue.js> and <module.wasm> are a `wasm-bindgen --target no-modules`
// build of ast-plotter-wasm made with its smoke-test feature, the glue the
// page inlines; <examples.json> is the examples array the build script
// writes; <worker.js> is the page's drawing Worker, run here with a stand-in
// for the Worker's scope; <date header> is a header with a date axis, whose
// tick spacing reads the clock.
//
// Every example, and the date axis, must draw as the page draws them, and
// after a panic traps an instance a new one must draw again, made as the
// page makes it: the glue evaluated afresh on the module already compiled.

import { readFileSync } from 'node:fs';

const [gluePath, wasmPath, examplesPath, workerPath, datePath] = process.argv.slice(2);
if (!datePath) {
  console.error(
    'usage: node plotter_smoke.mjs <glue.js> <module.wasm> <examples.json> <worker.js> <date header>',
  );
  process.exit(2);
}

// As index.html defines it: the glue, evaluated afresh at each call.
const makeWasmBindgen = new Function(`${readFileSync(gluePath, 'utf8')}\nreturn wasm_bindgen;`);
const plotterWorker = new Function(`${readFileSync(workerPath, 'utf8')}\nreturn plotterWorker;`)();
const module = await WebAssembly.compile(readFileSync(wasmPath));

let failures = 0;
function fail(message) {
  console.error(`FAIL: ${message}`);
  failures += 1;
}

// worker.js in a stand-in for a Worker's scope, sent the compiled module
// as the page sends it.
function startWorker() {
  const pending = new Map();
  let nextId = 0;
  const scope = {
    onmessage: null,
    postMessage(reply) {
      pending.get(reply.id)(reply);
      pending.delete(reply.id);
    },
  };
  plotterWorker(scope, makeWasmBindgen);
  scope.onmessage({ data: { module } });
  return (args) =>
    new Promise((resolve) => {
      const id = ++nextId;
      pending.set(id, resolve);
      scope.onmessage({ data: { id, args } });
    });
}

async function check(draw, name, header, settings, version) {
  const reply = await draw([header, settings, '', 600, 600, version, true]);
  if (reply.internal !== undefined) {
    fail(`${name}: internal error ${reply.internal}`);
  } else if (reply.svgError !== undefined) {
    fail(`${name}: ${reply.svgError}`);
  } else if (!reply.svg.startsWith('<?xml') || !reply.svg.trimEnd().endsWith('</svg>')) {
    fail(`${name}: not an SVG document`);
  } else {
    console.log(`ok ${name} (${reply.svg.length} bytes of SVG)`);
    return reply.svg;
  }
  return '';
}

const examples = JSON.parse(readFileSync(examplesPath, 'utf8'));
if (examples.length !== 8) fail(`expected 8 examples, found ${examples.length}`);

const draw = startWorker();
for (const example of examples) {
  await check(draw, example.name, example.header, example.settings, example.version);
}
const dateSvg = await check(draw, 'date axis', readFileSync(datePath, 'utf8'), '', ' ');
if (dateSvg && !/>\d{4}-\d\d-\d\d</.test(dateSvg)) fail('date axis: no date label');

const trapped = makeWasmBindgen();
await trapped({ module_or_path: module });
try {
  trapped.__trap_for_test();
  fail('__trap_for_test did not trap');
} catch (e) {
  if (!(e instanceof WebAssembly.RuntimeError)) fail(`__trap_for_test threw ${e}, not a trap`);
}
await check(startWorker(), 'a new instance after a trap', examples[0].header, examples[0].settings, ' ');

if (failures > 0) {
  console.error(`${failures} failure(s)`);
  process.exit(1);
}
