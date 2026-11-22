// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

// Smoke test of the FITS plotter's WebAssembly build, run by
// tools/build_plotter.sh --smoke.
//
//   node tools/plotter_smoke.mjs <glue dir> <examples.json>
//
// <glue dir> holds a `wasm-bindgen --target nodejs` build of ast-plotter-wasm
// made with its smoke-test feature; <examples.json> is the examples array the
// build script writes from web/plotter/examples.txt. Every example must draw
// its grid as the page draws it, and after a panic traps the instance a fresh
// instance must draw again, which is how the page recovers.

import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import path from 'node:path';

const require = createRequire(import.meta.url);
const [glueDir, examplesPath] = process.argv.slice(2);
if (!glueDir || !examplesPath) {
  console.error('usage: node plotter_smoke.mjs <glue dir> <examples.json>');
  process.exit(2);
}
const gluePath = require.resolve(path.resolve(glueDir, 'ast_plotter.js'));

// A fresh instance: the nodejs glue instantiates the module when it is
// loaded, so dropping it from the cache and loading it again gives a new one.
function instance() {
  delete require.cache[gluePath];
  return require(gluePath);
}

let failures = 0;
function fail(message) {
  console.error(`FAIL: ${message}`);
  failures += 1;
}

function draw(plotter, example) {
  const out = plotter.plot(
    example.header, example.settings, '', 600, 600, example.version, true);
  try {
    return { svg: out.svg(), error: out.svg_error() };
  } finally {
    out.free();
  }
}

const examples = JSON.parse(readFileSync(examplesPath, 'utf8'));
if (examples.length !== 8) fail(`expected 8 examples, found ${examples.length}`);

let plotter = instance();
for (const example of examples) {
  const { svg, error } = draw(plotter, example);
  if (error !== undefined) {
    fail(`${example.name}: ${error}`);
  } else if (!svg.startsWith('<?xml') || !svg.trimEnd().endsWith('</svg>')) {
    fail(`${example.name}: not an SVG document`);
  } else {
    console.log(`ok ${example.name} (${svg.length} bytes of SVG)`);
  }
}

let trapped = false;
try {
  plotter.__trap_for_test();
} catch (e) {
  trapped = e instanceof WebAssembly.RuntimeError;
  if (!trapped) fail(`__trap_for_test threw ${e}, not a WebAssembly trap`);
}
if (!trapped) fail('__trap_for_test did not trap');

plotter = instance();
const { error } = draw(plotter, examples[0]);
if (error !== undefined) {
  fail(`a fresh instance after the trap: ${error}`);
} else {
  console.log('ok a fresh instance draws after a trap');
}

if (failures > 0) {
  console.error(`${failures} failure(s)`);
  process.exit(1);
}
