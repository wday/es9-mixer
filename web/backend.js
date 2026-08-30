// Backend selection.
//
// The same frontend runs in two places: a browser against the WASM mock, and the desktop
// shell against a real ES-9 over MIDI. Rather than forking the UI, both are presented
// through one async interface and the right one is chosen at load time.
//
// Everything here is async even where the mock is synchronous, because the desktop
// backend crosses a process boundary and the UI must not be written twice.

const PRESET_KEY = 'es9-mixer.presets';

function readLocalPresets() {
  try {
    const raw = globalThis.localStorage?.getItem(PRESET_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch {
    // A browser with storage disabled should show no presets, not fail to start.
    return [];
  }
}

function writeLocalPresets(all) {
  try {
    globalThis.localStorage?.setItem(PRESET_KEY, JSON.stringify(all));
  } catch (e) {
    throw new Error(`could not save the preset: ${e}`);
  }
}

const isTauri = () =>
  typeof window !== 'undefined' &&
  (window.__TAURI_INTERNALS__ !== undefined || window.__TAURI__ !== undefined);

/// Wraps the WASM bridge, which drives an offline mock module.
async function createMockBackend() {
  const { default: init, Bridge } = await import('./pkg/es9_wasm.js');
  await init();
  const b = new Bridge();
  const wrap =
    (name) =>
    async (...args) =>
      b[name](...args);

  return {
    kind: 'mock',
    // The mock is always "connected"; there are no ports to choose.
    async ports() {
      return { inputs: [], outputs: [], connected: true, mock: true };
    },
    async connect() {
      return { connected: true, mock: true };
    },
    async meters() {
      return null;
    },
    snapshot: wrap('snapshot'),
    monitor: wrap('monitor'),
    clearMonitor: wrap('clearMonitor'),
    setMacro: wrap('setMacro'),
    setRawMix: wrap('setRawMix'),
    setCapture: wrap('setCapture'),
    setOutput: wrap('setOutput'),
    setLink: wrap('setLink'),
    setOptions: wrap('setOptions'),
    setSmoothing: wrap('setSmoothing'),
    setDcBlock: wrap('setDcBlock'),
    setDcOffset: wrap('setDcOffset'),
    resetDefaults: wrap('resetDefaults'),

    // The mock has no filesystem, so presets live in this browser's local storage. They
    // hold the same `08H` dump bytes the desktop build writes to `.syx` files, so the two
    // stores are the same format even though they sit in different places.
    async presetList() {
      return readLocalPresets().map(({ name, bytes, savedAt }) => ({
        name,
        bytes,
        savedAt,
        loadable: true,
        problem: null,
      }));
    },
    async presetSave(name) {
      const clean = String(name ?? '').trim();
      if (!clean) throw new Error('a preset needs a name');
      const dump = Array.from(b.configDump());
      const all = readLocalPresets().filter((p) => p.name !== clean);
      all.push({
        name: clean,
        dump,
        bytes: dump.length,
        savedAt: Math.floor(Date.now() / 1000),
      });
      writeLocalPresets(all);
    },
    async presetLoad(name) {
      const found = readLocalPresets().find((p) => p.name === name);
      if (!found) throw new Error(`no preset called ${name}`);
      b.loadConfigDump(Uint8Array.from(found.dump));
    },
    async presetDelete(name) {
      writeLocalPresets(readLocalPresets().filter((p) => p.name !== name));
    },
    setMidiChannels: wrap('setMidiChannels'),
    undo: wrap('undo'),
    redo: wrap('redo'),
    beginEdit: wrap('beginEdit'),
    save: wrap('save'),
  };
}

/// Talks to the Rust backend in the desktop shell, which holds the real MIDI link.
async function createTauriBackend() {
  const invoke = window.__TAURI__?.core?.invoke ?? window.__TAURI_INTERNALS__.invoke;
  const call =
    (name) =>
    (...args) =>
      invoke(name, argsFor(name, args));

  return {
    kind: 'device',
    ports: call('ports'),
    connect: call('connect'),
    meters: call('meters'),
    snapshot: call('snapshot'),
    monitor: call('monitor'),
    clearMonitor: call('clear_monitor'),
    setMacro: call('set_macro'),
    setRawMix: call('set_raw_mix'),
    setCapture: call('set_capture'),
    setOutput: call('set_output'),
    setLink: call('set_link'),
    setOptions: call('set_options'),
    setSmoothing: call('set_smoothing'),
    setDcBlock: call('set_dc_block'),
    setDcOffset: call('set_dc_offset'),
    resetDefaults: call('reset_defaults'),
    presetList: call('preset_list'),
    presetSave: call('preset_save'),
    presetLoad: call('preset_load'),
    presetDelete: call('preset_delete'),
    setMidiChannels: call('set_midi_channels'),
    undo: call('undo'),
    redo: call('redo'),
    beginEdit: call('begin_edit'),
    save: call('save'),
  };
}

// Tauri commands take named arguments, so positional calls are mapped by name here. This
// table is the one place the two backends' calling conventions differ.
const ARG_NAMES = {
  connect: ['input', 'output'],
  set_macro: ['cc', 'value'],
  set_raw_mix: ['mix', 'channel', 'value'],
  set_capture: ['block', 'channel', 'wire'],
  set_output: ['block', 'channel', 'wire'],
  set_link: ['family', 'evenIndex', 'on'],
  set_options: ['mixer2', 'midiThru'],
  set_smoothing: ['mix', 'on'],
  set_dc_block: ['bit', 'on'],
  set_dc_offset: ['channel', 'offset'],
  reset_defaults: ['standalone'],
  preset_save: ['name'],
  preset_load: ['name'],
  preset_delete: ['name'],
  set_midi_channels: ['usb', 'din'],
  begin_edit: ['standalone'],
};

function argsFor(name, args) {
  const names = ARG_NAMES[name];
  if (!names) return {};
  const out = {};
  names.forEach((n, i) => {
    out[n] = args[i];
  });
  return out;
}

export async function createBackend() {
  return isTauri() ? createTauriBackend() : createMockBackend();
}
