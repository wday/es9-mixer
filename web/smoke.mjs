// End-to-end smoke test of the WASM bridge, run headlessly under Node.
//
// Covers the whole stack -- protocol codec, device model, mock module, presentation
// layer, and the JS binding -- without a browser or any hardware. Run with
// `./scripts/smoke.sh`.

import { createRequire } from 'node:module';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const require = createRequire(import.meta.url);
const here = path.dirname(fileURLToPath(import.meta.url));
const { Bridge } = require(path.join(here, '..', 'target', 'es9node', 'es9_wasm.js'));

const b = new Bridge();
let s = b.snapshot();

// --- snapshot shape ---------------------------------------------------------
assert.equal(s.status.version, 'ES-9 1.3.1');
assert.equal(s.status.sampleRate, 48000);
assert.equal(s.status.editing, 'hosted');
assert.equal(s.status.dirty, false);
assert.equal(s.status.mixer2, true, 'standalone preset enables mixer 2');
assert.equal(s.mixes.length, 16, 'sixteen mixes with mixer 2 on');
assert.equal(s.ccRows.length, 128);
assert.equal(s.routing.capture.length, 32);
assert.equal(s.routing.outputs.length, 32);

// --- the standalone preset wires analogue inputs to the mixer ---------------
assert.equal(s.mixes[0].strips[0].label, 'Input 1');
assert.equal(s.mixes[0].destination, 'Main Out L');
assert.equal(s.mixes[0].strips[0].levelCc, 0);
console.log('mix 1 strip 1:', s.mixes[0].strips[0].label, '->', s.mixes[0].destination);

// --- a fader move round-trips through the mock ------------------------------
b.setMacro(0, 110);
s = b.snapshot();
assert.equal(s.mixes[0].strips[0].level, 110);
assert.ok(Math.abs(s.mixes[0].strips[0].levelDb - 3.5) < 0.01, 'dB conversion');
assert.equal(s.status.dirty, true);

// --- undo restores it -------------------------------------------------------
assert.equal(b.undo(), true);
s = b.snapshot();
assert.equal(s.mixes[0].strips[0].level, 103);
assert.equal(s.canRedo, true);

// --- linking a mix pair reports every rewritten CC --------------------------
const mixFamily = s.links.findIndex((l) => l.label === 'MIX 1/2');
assert.ok(mixFamily >= 0, 'MIX 1/2 link offered');
const link = s.links[mixFamily];
const changes = b.setLink(link.family, link.evenIndex, true);
assert.equal(changes.length, 16, 'one link toggle rewrites sixteen CCs');
const cc8 = changes.find((c) => c.cc === 8);
assert.equal(cc8.before, 'Mix 2, ch 1, level');
assert.equal(cc8.after, 'Mix 1/2, ch 1, pan');
console.log('link toggle rewrote', changes.length, 'CCs; CC8:', cc8.before, '->', cc8.after);

// --- the merged strip now has pan ------------------------------------------
s = b.snapshot();
assert.equal(s.mixes.length, 15, 'mixes 1 and 2 merged');
assert.equal(s.mixes[0].name, 'Mix 1/2');
assert.equal(s.mixes[0].destination, 'Main Out L/R');
assert.equal(s.mixes[0].strips[0].panCc, 8);

// --- stereo source collapses two strips ------------------------------------
const inputLink = s.links.find((l) => l.label === 'Input 1/2');
b.setLink(inputLink.family, inputLink.evenIndex, true);
s = b.snapshot();
assert.equal(s.mixes[0].strips[0].label, 'Input 1/2');
assert.equal(s.mixes[0].strips.length, 7, 'two channels became one strip');

// --- routing: clicking a cell moves the assignment -------------------------
const busSource = s.sources.find((x) => x.label === 'Bus 5');
b.setCapture(0, 0, busSource.wire);
s = b.snapshot();
assert.equal(s.routing.capture[0].label, 'Bus 5');

// --- the guarded standalone workflow ---------------------------------------
b.beginEdit(true);
s = b.snapshot();
assert.equal(s.status.editing, 'standalone');
assert.equal(s.status.dirty, false, 'loading a slot is not a change');
assert.equal(s.canUndo, false, 'history resets when switching slots');

const log = b.monitor();
const restore = log.find((e) => e.description.startsWith('restore from flash'));
assert.ok(restore, 'the slot was loaded before editing');
assert.equal(restore.description, 'restore from flash, standalone');
console.log('monitor:', log.length, 'messages;', restore.description);

// --- monitor decodes a macro write as its CC number ------------------------
b.setMacro(8, 40);
const macroLine = b.monitor().findLast((e) => e.description.startsWith('macro mix'));
assert.equal(macroLine.description, 'macro mix: CC 8 = 40');

// --- input DC blocking -----------------------------------------------------
s = b.snapshot();
assert.equal(s.dcFilters.length, 7, 'seven filters for fourteen inputs');

// Every input is covered exactly once.
const covered = s.dcFilters.flatMap((f) => f.inputs).sort((x, y) => x - y);
assert.deepEqual(covered, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14]);

// The pairing users get wrong, asserted by name: inputs 3 and 8 share a filter, and it
// is the only non-adjacent pair, so it is the only one flagged.
const odd = s.dcFilters.filter((f) => f.surprising);
assert.equal(odd.length, 1);
assert.equal(odd[0].label, 'Inputs 3/8');
assert.deepEqual(odd[0].inputs, [3, 8]);

// Toggling one filter must not disturb its neighbours.
const before = s.dcFilters.map((f) => f.on);
b.setDcBlock(odd[0].bit, !before[odd[0].bit]);
s = b.snapshot();
assert.equal(s.dcFilters[odd[0].bit].on, !before[odd[0].bit], 'the filter toggled');
for (let i = 0; i < s.dcFilters.length; i += 1) {
  if (i === odd[0].bit) continue;
  assert.equal(s.dcFilters[i].on, before[i], `filter ${i} was disturbed`);
}

// The write reached the module, not just the model.
const dcLine = b.monitor().findLast((e) => e.description.startsWith('DC blocking'));
assert.ok(dcLine, 'the DC-blocking write appears in the monitor');

// Undo restores that one filter.
assert.equal(b.undo(), true);
s = b.snapshot();
assert.deepEqual(
  s.dcFilters.map((f) => f.on),
  before,
  'undo restores the filter it changed',
);
console.log('dc blocking:', s.dcFilters.map((f) => `${f.label}=${f.on ? 'on' : 'off'}`).join(', '));

// --- output dc offsets ---------------------------------------------------
s = b.snapshot();
assert.equal(s.dcOffsets.length, 8, 'eight analogue outputs');
assert.equal(s.dcOffsets[0].label, 'Output 1');

// The offset is implemented with the mixer hardware, so it only does anything on an
// output a mix is routed to. The standalone preset routes mixes 5-8 to outputs 1-4.
const effective = s.dcOffsets.filter((o) => o.effective).map((o) => o.label);
assert.deepEqual(effective, ['Output 1', 'Output 2', 'Output 3', 'Output 4']);
assert.equal(s.dcOffsets[0].fedBy, 'Mix 5');
assert.equal(s.dcOffsets[4].effective, false, 'Output 5 has no mixer routed to it');

b.setDcOffset(0, -1200);
s = b.snapshot();
assert.equal(s.dcOffsets[0].offset, -1200, 'the offset reached the module');

const offsetLine = b.monitor().findLast((e) => e.description.startsWith('set DC offset'));
assert.ok(offsetLine, 'the offset write appears in the monitor');
assert.equal(offsetLine.description, 'set DC offset: output 1 = -1200');

assert.equal(b.undo(), true);
s = b.snapshot();
assert.equal(s.dcOffsets[0].offset, 0, 'undo restores the offset');
console.log('dc offsets: effective on', effective.join(', '));

// --- presets and factory defaults ------------------------------------------
// A configuration dump round-trips through the same bytes both builds store.
const savedDump = b.configDump();
assert.equal(savedDump.length, 2313, 'a dump is 2313 bytes');

s = b.snapshot();
const beforeReset = JSON.stringify(s.routing);

// Resetting to hosted defaults changes the mixer sources away from the analogue inputs
// the standalone preset uses.
b.resetDefaults(false);
s = b.snapshot();
assert.notEqual(JSON.stringify(s.routing), beforeReset, 'hosted defaults changed routing');

// It is one undoable step, not a point of no return.
assert.equal(b.undo(), true);
s = b.snapshot();
assert.equal(JSON.stringify(s.routing), beforeReset, 'undo restored the configuration');

// And a stored dump reloads to exactly what it captured.
b.resetDefaults(false);
b.loadConfigDump(savedDump);
s = b.snapshot();
assert.equal(JSON.stringify(s.routing), beforeReset, 'the saved dump reloaded exactly');

assert.throws(() => b.loadConfigDump(Uint8Array.from([0xf0, 0xf7])), 'a bad dump is refused');
console.log('presets: dump round-tripped, reset undone');

console.log('\nAll bridge smoke tests passed.');
