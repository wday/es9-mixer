// ES-9 mixer frontend.
//
// Backend-agnostic: `backend.js` supplies either the WASM mock (in a browser) or the
// desktop shell's real MIDI link (under Tauri). Every backend call is awaited, because
// the desktop one crosses a process boundary.

import { createBackend } from './backend.js';

let bridge;
let snap;
// The monitor log and meter levels are fetched alongside the snapshot so that every
// render function can stay synchronous.
let log = [];
let meters = null;
let tab = 'mixer';
let focusedMix = 0;

const $ = (id) => document.getElementById(id);

// Pan centre is macro 64, measured on hardware (checklist H4). The reference tool reads
// it as 63, which sends one step left when asked for centre; do not copy that.
const PAN_CENTRE = 64;
const PAN_MAX = 63;
const panToMacro = (pan) => Math.max(-PAN_MAX, Math.min(PAN_MAX, pan)) + PAN_CENTRE;

// Range the module accepts for an output DC offset. The manual gives no units and the
// official tool shows the bare number, so this does too rather than inventing volts.
const DC_OFFSET_MAX = 3176;

// ---------------------------------------------------------------- send coalescing
//
// A drag fires an input event per pointer move. Sending each one floods the MIDI link
// and makes the monitor unreadable, so the latest value per control is collapsed into
// one send per animation frame.
const pendingSends = new Map();
let frame = null;

// True while a pointer is held on a control. The background refresh below must not
// re-render mid-drag: rebuilding the DOM under a moving thumb loses the drag.
let interacting = false;
if (typeof window !== 'undefined') {
  window.addEventListener('pointerdown', () => {
    interacting = true;
  });
  window.addEventListener('pointerup', () => {
    interacting = false;
  });
  window.addEventListener('pointercancel', () => {
    interacting = false;
  });
}

function queueSend(key, fn) {
  pendingSends.set(key, fn);
  if (frame === null) frame = requestAnimationFrame(flushSends);
}

async function flushSends() {
  frame = null;
  const sends = [...pendingSends.values()];
  pendingSends.clear();
  for (const fn of sends) await fn();
  await refresh();
}

// One fetch of everything the current tab needs, then a synchronous render. Keeping the
// await surface here means no render function has to be async.
async function refresh() {
  try {
    snap = await bridge.snapshot();
  } catch (e) {
    // A backend that stops answering must say so rather than leaving the last good
    // snapshot on screen, which would read as a live mixer when it is a photograph.
    showNotice(`Backend error: ${e}`);
    throw e;
  }
  if (tab === 'monitor') log = await bridge.monitor();
  if (tab === 'presets') presets = await bridge.presetList();
  if (bridge.kind === 'device') meters = await bridge.meters();
  // A render that throws would otherwise leave the previous frame on screen, which reads
  // as a working mixer showing stale values — the worst possible failure for this tool.
  try {
    render();
  } catch (e) {
    showNotice(`Render failed: ${e && e.stack ? e.stack : e}`);
    throw e;
  }
}

// ---------------------------------------------------------------- notices

/// Shows a plain message in the notice bar.
function showNotice(text) {
  const box = $('notice');
  box.hidden = false;
  box.textContent = text;
}

function showCcChanges(changes, what) {
  const box = $('notice');
  if (!changes || changes.length === 0) {
    box.hidden = true;
    return;
  }
  box.hidden = false;
  box.innerHTML = '';
  const h = document.createElement('h4');
  h.textContent = `${what} rewrote ${changes.length} MIDI CC ${
    changes.length === 1 ? 'assignment' : 'assignments'
  }`;
  const p = document.createElement('div');
  p.textContent =
    'CC numbers are fixed in hardware. Any controller mapped to these now does something different.';
  const ul = document.createElement('ul');
  for (const c of changes) {
    const li = document.createElement('li');
    li.textContent = `CC ${c.cc}: ${c.before ?? 'unassigned'} → ${c.after ?? 'unassigned'}`;
    ul.append(li);
  }
  box.append(h, p, ul);
}

// ---------------------------------------------------------------- header

function renderHeader() {
  const s = snap.status;
  const bits = [];
  if (s.version) bits.push(`<b>${s.version}</b>`);
  if (s.sampleRate) bits.push(`${(s.sampleRate / 1000).toFixed(1)} kHz`);
  if (s.dspLoad != null) bits.push(`DSP ${s.dspLoad.toFixed(1)}%`);
  // Options bit 0 chooses what routing block 3 *is*, so name the block rather than
  // announcing a feature: "S/PDIF on" reads as "S/PDIF works", which the mode bit does
  // not promise. When the block is S/PDIF, say what it is actually carrying.
  if (s.mixer2) {
    bits.push('Block 3: Mixer 2');
  } else if (s.spdif) {
    const inbound = s.spdif.inputTo
      ? `in \u2192 ${s.spdif.inputTo}`
      : 'in not captured';
    bits.push(
      `Block 3: S/PDIF <span class="dim">out \u2190 ${s.spdif.outputFrom} &middot; ${inbound}</span>`,
    );
  } else {
    bits.push('Block 3: S/PDIF');
  }
  $('status').innerHTML = bits.join('</span><span>').replace(/^/, '<span>') + '</span>';

  const slot = $('slot');
  slot.className = 'slot' + (s.dirty ? ' dirty' : '');
  slot.innerHTML = `<span class="dot"></span>editing <b>${s.editing}</b>${
    s.dirty ? ' &middot; unsaved' : ''
  }`;

  $('undo').disabled = !snap.canUndo;
  $('redo').disabled = !snap.canRedo;
  $('save').textContent = `Save to ${s.editing}`;
}

// ---------------------------------------------------------------- mixer

function renderMixList() {
  const list = $('mixlist');
  list.innerHTML = '';
  for (const m of snap.mixes) {
    const b = document.createElement('button');
    b.className = m.index === focusedMix ? 'on' : '';
    b.innerHTML = `<div class="name">${m.name}</div><div class="dest">${m.destination}</div>`;
    b.onclick = () => {
      focusedMix = m.index;
      render();
    };
    list.append(b);
  }
}

function renderConsole() {
  const host = $('console');
  host.innerHTML = '';
  const mix = snap.mixes.find((m) => m.index === focusedMix) ?? snap.mixes[0];
  if (!mix) return;
  focusedMix = mix.index;

  const head = document.createElement('div');
  head.className = 'console-head';
  head.innerHTML = `<h2>${mix.name}</h2><span class="dest">&rarr; ${mix.destination}</span>`;
  const smooth = document.createElement('label');
  const sc = document.createElement('input');
  sc.type = 'checkbox';
  sc.checked = mix.smoothing;
  sc.onchange = async () => {
    await bridge.setSmoothing(mix.index, sc.checked);
    await refresh();
  };
  smooth.append(sc, document.createTextNode('gain smoothing'));
  head.append(smooth);

  const strips = document.createElement('div');
  strips.className = 'strips';
  for (const s of mix.strips) strips.append(renderStrip(s));

  host.append(head, strips);
}

function renderStrip(s) {
  const el = document.createElement('div');
  el.className = 'strip';

  const src = document.createElement('div');
  src.className = 'src';
  src.textContent = s.label;

  const fader = document.createElement('input');
  fader.type = 'range';
  fader.className = 'fader';
  fader.min = 0;
  fader.max = 127;
  fader.value = s.level;

  const db = document.createElement('input');
  db.className = 'db';
  db.value = s.levelDb == null ? '-inf' : formatDb(s.levelDb);

  fader.oninput = () => {
    const v = Number(fader.value);
    db.value = v === 0 ? '-inf' : formatDb(macroToDbApprox(v));
    queueSend(`level:${s.levelCc}`, () => bridge.setMacro(s.levelCc, v));
  };
  // Double-click resets to unity, matching the reference tool's behaviour.
  fader.ondblclick = async () => {
    fader.value = 103;
    await bridge.setMacro(s.levelCc, 103);
    refresh();
  };
  // Typing an exact value is the thing the original tool made impossible.
  db.onchange = async () => {
    const v = dbToMacro(db.value);
    if (v !== null) await bridge.setMacro(s.levelCc, v);
    refresh();
  };

  const ccBadge = document.createElement('span');
  ccBadge.className = 'cc';
  ccBadge.textContent = `CC ${s.levelCc}`;
  ccBadge.title = 'MIDI CC controlling this fader';

  el.append(src, fader, db, ccBadge);

  if (s.panCc != null) {
    const pan = document.createElement('input');
    pan.type = 'range';
    pan.className = 'pan';
    // Centre is macro 64, so the symmetric travel is 63 steps either side.
    pan.min = -PAN_MAX;
    pan.max = PAN_MAX;
    pan.value = s.pan ?? 0;

    const val = document.createElement('div');
    val.className = 'panval';
    val.textContent = formatPan(s.pan ?? 0);

    pan.oninput = () => {
      const v = Number(pan.value);
      val.textContent = formatPan(v);
      queueSend(`pan:${s.panCc}`, () => bridge.setMacro(s.panCc, panToMacro(v)));
    };
    pan.ondblclick = async () => {
      pan.value = 0;
      await bridge.setMacro(s.panCc, PAN_CENTRE);
      refresh();
    };

    const panBadge = document.createElement('span');
    panBadge.className = 'cc pan';
    panBadge.textContent = `CC ${s.panCc}`;
    panBadge.title = 'MIDI CC controlling pan';
    el.append(pan, val, panBadge);
  }

  // A read-only readout of the gains the hardware is actually running. Spike S1 settled
  // that these are driven by the macro level and replaced whenever it moves, so they are
  // shown but never made editable — an editable raw control would not hold its value.
  const raw = document.createElement('details');
  raw.className = 'raw';
  raw.innerHTML =
    `<summary>raw</summary><div class="vals">${s.raw.join(', ')}</div>` +
    `<div class="caveat">Read-only. The macro fader above drives these, and overwrites them whenever it moves.</div>`;
  el.append(raw);

  return el;
}

// The Rust side owns the real conversion; this mirrors it for live drag feedback only.
function macroToDbApprox(v) {
  if (v === 0) return -Infinity;
  return v >= 55 ? -24 + 0.5 * (v - 55) : -72 + ((v - 1) * 48) / 54;
}

function dbToMacro(text) {
  const t = String(text).trim().toLowerCase();
  if (t === '-inf' || t === 'inf' || t === '') return 0;
  const db = Number.parseFloat(t);
  if (Number.isNaN(db)) return null;
  if (db < -72) return 0;
  const v = db >= -24 ? Math.round((db + 24) * 2 + 55) : Math.round(((db + 72) * 54) / 48 + 1);
  return Math.min(127, Math.max(0, v));
}

const formatDb = (db) => (db > 0 ? '+' : '') + db.toFixed(1);
const formatPan = (p) => (p === 0 ? 'C' : p < 0 ? `${-p}L` : `${p}R`);

// ---------------------------------------------------------------- routing

function renderMatrix(hostId, channels, choices, apply) {
  const host = $(hostId);
  host.innerHTML = '';
  const table = document.createElement('table');
  table.className = 'matrix';

  const thead = document.createElement('thead');
  const hr = document.createElement('tr');
  hr.append(el('th', 'corner', ''));
  channels.forEach((c, i) => {
    const th = el('th', i % 8 === 7 ? 'blockgap' : '', String(c.channel + 1));
    th.title = `Block ${c.block}, channel ${c.channel + 1}`;
    hr.append(th);
  });
  thead.append(hr);

  const blockRow = document.createElement('tr');
  blockRow.append(el('th', 'corner', ''));
  for (let b = 0; b < 4; b++) {
    const th = el('th', 'blockgap', blockName(b, hostId));
    th.colSpan = 8;
    blockRow.append(th);
  }
  thead.append(blockRow);

  const tbody = document.createElement('tbody');
  let lastGroup = null;
  for (const choice of choices) {
    const group = choice.label.replace(/\s*\d+.*$/, '') || choice.label;
    if (group !== lastGroup) {
      lastGroup = group;
      const gr = document.createElement('tr');
      gr.className = 'group';
      const th = el('th', 'rowhead', group);
      th.colSpan = channels.length + 1;
      gr.append(th);
      tbody.append(gr);
    }
    const tr = document.createElement('tr');
    tr.append(el('th', 'rowhead', choice.label));
    channels.forEach((c, i) => {
      const td = el('td', 'cell' + (i % 8 === 7 ? ' blockgap' : ''), '');
      if (c.wire === choice.wire) td.classList.add('on');
      td.title = `${choice.label} → block ${c.block} channel ${c.channel + 1}`;
      td.onclick = async () => {
        await apply(c.block, c.channel, choice.wire);
        await refresh();
      };
      tr.append(td);
    });
    tbody.append(tr);
  }
  table.append(thead, tbody);
  host.append(table);
}

function blockName(b, hostId) {
  const capture = hostId === 'matrix-capture';
  if (b === 0) return capture ? 'USB in 1-8' : 'USB out 1-8';
  if (b === 1) return capture ? 'USB in 9-16' : 'USB out 9-16';
  if (b === 2) return capture ? 'Mixer 1 inputs' : 'Mixes 1-8';
  return capture ? 'Mixer 2 inputs' : 'Mixes 9-16';
}

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text) e.textContent = text;
  return e;
}

function renderCapture() {
  renderMatrix('matrix-capture', snap.routing.capture, snap.sources, (b, c, w) =>
    bridge.setCapture(b, c, w),
  );
}

function renderOutputs() {
  renderMatrix('matrix-outputs', snap.routing.outputs, snap.destinations, (b, c, w) =>
    bridge.setOutput(b, c, w),
  );
}

// ---------------------------------------------------------------- output dc offsets

function renderDcOffsets() {
  const host = $('dcoffsets');
  host.innerHTML = '';
  for (const o of snap.dcOffsets) {
    const row = document.createElement('div');
    row.className = 'dcoffset' + (o.effective ? '' : ' inert');

    const label = document.createElement('div');
    label.className = 'dolabel';
    label.textContent = o.label;

    const slider = document.createElement('input');
    slider.type = 'range';
    slider.className = 'doslider';
    slider.min = -DC_OFFSET_MAX;
    slider.max = DC_OFFSET_MAX;
    slider.value = o.offset;
    slider.disabled = !o.effective;

    const value = document.createElement('div');
    value.className = 'dovalue';
    value.textContent = o.offset;

    slider.oninput = () => {
      const v = Number(slider.value);
      value.textContent = v;
      queueSend(`dc:${o.channel}`, () => bridge.setDcOffset(o.channel, v));
    };
    slider.ondblclick = async () => {
      slider.value = 0;
      value.textContent = '0';
      await bridge.setDcOffset(o.channel, 0);
      await refresh();
    };

    const note = document.createElement('div');
    note.className = 'donote';
    // Say why it is inert, not just that it is: the fix is a routing change, and the
    // user has no way to guess that from a greyed-out slider.
    note.textContent = o.effective
      ? `fed by ${o.fedBy}`
      : 'no mix routed here — offset has no effect';

    row.append(label, slider, value, note);
    host.append(row);
  }
}

function renderLinks() {
  const host = $('links');
  host.innerHTML = '';
  for (const l of snap.links) {
    const label = document.createElement('label');
    const cb = document.createElement('input');
    cb.type = 'checkbox';
    cb.checked = l.linked;
    cb.onchange = async () => {
      const changes = await bridge.setLink(l.family, l.evenIndex, cb.checked);
      await refresh();
      showCcChanges(changes, `Linking ${l.label}`);
    };
    label.append(cb, document.createTextNode(l.label));
    host.append(label);
  }
}

// ---------------------------------------------------------------- input dc blocking

function renderDcFilters() {
  const host = $('dcfilters');
  host.innerHTML = '';
  for (const f of snap.dcFilters) {
    const card = document.createElement('label');
    card.className = 'dcfilter' + (f.on ? ' on' : '');

    const cb = document.createElement('input');
    cb.type = 'checkbox';
    cb.checked = f.on;
    cb.onchange = async () => {
      await bridge.setDcBlock(f.bit, cb.checked);
      await refresh();
    };

    const text = document.createElement('div');
    text.className = 'dctext';
    const name = document.createElement('div');
    name.className = 'dcname';
    name.textContent = f.label;
    const state = document.createElement('div');
    state.className = 'dcstate';
    // Say what the setting means rather than only whether it is on: "on" and "off" do
    // not tell you which one breaks your CV.
    state.textContent = f.on ? 'filtered — for audio' : 'unfiltered — passes CV';
    text.append(name, state);

    card.append(cb, text);

    // Inputs 3/8 are the pair someone will get wrong, so the card says so itself.
    if (f.surprising) {
      const warn = document.createElement('div');
      warn.className = 'dcwarn';
      warn.textContent = `not adjacent: this switches inputs ${f.inputs[0]} and ${f.inputs[1]} together`;
      card.append(warn);
    }
    host.append(card);
  }
}

// ---------------------------------------------------------------- meters

// Meter channel i is USB capture channel i, so the routing says what each one is
// carrying. That is the whole point: an unlabelled meter bridge tells you a signal
// exists, not what it is.
function meterLabel(i) {
  const c = snap?.routing?.capture?.[i];
  return c ? c.label : `Capture ${i + 1}`;
}

// -72 dBFS is the floor the module's own gain scale bottoms out at, so it is the floor
// here too rather than an arbitrary one.
const METER_FLOOR = -72;

// Below this the reading is indistinguishable from converter noise, so highlighting it
// would cry wolf on every channel.
const DC_VISIBLE = 0.001;
const meterFraction = (db) =>
  db == null || db <= METER_FLOOR ? 0 : Math.min(1, (db - METER_FLOOR) / -METER_FLOOR);

function renderMeters() {
  const host = $('meters');
  if (!host) return;
  const status = $('meters-status');

  if (!meters) {
    host.innerHTML = '';
    if (status) status.textContent = 'No audio device open.';
    return;
  }
  if (status) {
    status.textContent = `${meters.channels.length} channels at ${meters.sampleRate} Hz via ${meters.host}.`;
  }

  // Built once, then updated in place: rebuilding sixteen bars at meter rate would throw
  // away the browser's own compositing and make the whole page flicker.
  if (host.children.length !== meters.channels.length) {
    host.innerHTML = '';
    for (let i = 0; i < meters.channels.length; i += 1) {
      const row = document.createElement('div');
      row.className = 'meter';
      row.innerHTML =
        `<div class="mlabel"></div>` +
        `<div class="mbar"><div class="mfill"></div><div class="mhold"></div></div>` +
        `<div class="mdb"></div>` +
        `<div class="mdc"></div>`;
      host.append(row);
    }
  }

  meters.channels.forEach((m, i) => {
    const row = host.children[i];
    row.classList.toggle('clip', m.clipped);
    row.querySelector('.mlabel').textContent = meterLabel(i);
    row.querySelector('.mfill').style.width = `${meterFraction(m.peakDb) * 100}%`;
    row.querySelector('.mhold').style.left = `${meterFraction(m.holdDb) * 100}%`;
    row.querySelector('.mdb').textContent =
      m.peakDb == null || m.peakDb <= METER_FLOOR ? '\u2013\u221e' : m.peakDb.toFixed(1);

    // DC is the whole point on a DC-coupled module: peak and RMS discard sign, so an
    // idle channel sitting at a constant voltage looks like silence to both of them.
    const dc = m.dc ?? 0;
    const dcCell = row.querySelector('.mdc');
    dcCell.textContent = (dc >= 0 ? '+' : '') + dc.toFixed(4);
    dcCell.classList.toggle('offset', Math.abs(dc) >= DC_VISIBLE);
  });
}

// ---------------------------------------------------------------- presets

let presets = [];

// Destructive actions arm on the first click and fire on the second. A webview dialog
// would need a plugin, and an unconfirmed button that replaces the whole configuration is
// not something to leave one stray click away.
const armed = new Map();

function armButton(button, key, label, confirmLabel, run) {
  const disarm = () => {
    armed.delete(key);
    button.textContent = label;
    button.classList.remove('armed');
  };
  button.textContent = armed.has(key) ? confirmLabel : label;
  button.classList.toggle('armed', armed.has(key));
  button.onclick = async () => {
    if (!armed.has(key)) {
      armed.set(key, setTimeout(() => {
        disarm();
      }, 4000));
      button.textContent = confirmLabel;
      button.classList.add('armed');
      return;
    }
    clearTimeout(armed.get(key));
    disarm();
    await run();
  };
}

function renderDefaults() {
  const host = $('defaults');
  host.innerHTML = '';
  for (const [standalone, name, detail] of [
    [false, 'hosted', 'mixer inputs are the USB channels, S/PDIF enabled'],
    [true, 'standalone', 'mixer inputs are the analogue inputs, second mixer enabled'],
  ]) {
    const row = document.createElement('div');
    row.className = 'defaultrow';
    const text = document.createElement('div');
    text.className = 'defaulttext';
    text.innerHTML = `<div class="defaultname">Reset to ${name} defaults</div><div class="defaultdetail">${detail}</div>`;
    const button = document.createElement('button');
    armButton(button, `reset:${name}`, 'Reset', 'Confirm reset', async () => {
      try {
        await bridge.resetDefaults(standalone);
        showNotice(`Reset to ${name} defaults. Nothing is saved to flash until you press Save.`);
      } catch (e) {
        showNotice(`Reset failed: ${e}`);
      }
      await refresh();
    });
    row.append(text, button);
    host.append(row);
  }
}

function renderPresetList() {
  const host = $('presetlist');
  host.innerHTML = '';
  if (presets.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'hint';
    empty.textContent = 'No presets saved yet.';
    host.append(empty);
    return;
  }
  for (const p of presets) {
    const row = document.createElement('div');
    row.className = 'presetrow' + (p.loadable ? '' : ' broken');

    const text = document.createElement('div');
    text.className = 'presettext';
    const when = p.savedAt ? new Date(p.savedAt * 1000).toLocaleString() : 'unknown date';
    text.innerHTML =
      `<div class="presetname"></div>` +
      `<div class="presetmeta">${when} &middot; ${p.bytes} bytes</div>`;
    // Set as text, not markup: a preset name is user input and ends up on screen.
    text.querySelector('.presetname').textContent = p.name;
    if (!p.loadable) {
      const why = document.createElement('div');
      why.className = 'presetproblem';
      why.textContent = p.problem ?? 'this file will not load';
      text.append(why);
    }

    const loadBtn = document.createElement('button');
    loadBtn.disabled = !p.loadable;
    armButton(loadBtn, `load:${p.name}`, 'Load', 'Confirm load', async () => {
      try {
        await bridge.presetLoad(p.name);
        showNotice(`Loaded "${p.name}" and verified it against the module.`);
      } catch (e) {
        showNotice(`Load failed: ${e}`);
      }
      await refresh();
    });

    const delBtn = document.createElement('button');
    armButton(delBtn, `del:${p.name}`, 'Delete', 'Confirm delete', async () => {
      try {
        await bridge.presetDelete(p.name);
      } catch (e) {
        showNotice(`Delete failed: ${e}`);
      }
      await refresh();
    });

    row.append(text, loadBtn, delBtn);
    host.append(row);
  }
}

function renderPresets() {
  renderDefaults();
  renderPresetList();
}

// ---------------------------------------------------------------- cc map + monitor

function renderCcMap() {
  const host = $('ccgrid');
  host.innerHTML = '';
  for (const row of snap.ccRows) {
    const d = document.createElement('div');
    d.className = 'ccrow' + (row.target ? '' : ' unassigned');
    d.innerHTML = `<span class="n">CC ${row.cc}</span><span class="t">${
      row.target ?? 'unassigned'
    }</span>`;
    host.append(d);
  }
}

function renderMonitor() {
  const host = $('log');
  host.innerHTML = '';
  for (const e of log) {
    const row = document.createElement('div');
    row.className = 'row';
    row.innerHTML =
      `<span class="dir ${e.direction}">${e.direction.toUpperCase()}</span>` +
      `<span class="hex">${e.hex}</span>` +
      `<span class="desc">${e.description}</span>`;
    host.append(row);
  }
  host.scrollTop = host.scrollHeight;
}

// ---------------------------------------------------------------- shell

function render() {
  renderHeader();
  if (tab === 'mixer') {
    renderMixList();
    renderConsole();
  } else if (tab === 'capture') {
    renderCapture();
  } else if (tab === 'outputs') {
    renderOutputs();
  } else if (tab === 'analogue') {
    renderDcFilters();
    renderDcOffsets();
  } else if (tab === 'meters') {
    renderMeters();
  } else if (tab === 'presets') {
    renderPresets();
  } else if (tab === 'ccmap') {
    renderLinks();
    renderCcMap();
  } else {
    renderMonitor();
  }
}

async function selectTab(next) {
  tab = next;
  for (const b of $('tabs').children) b.classList.toggle('on', b.dataset.tab === next);
  for (const name of [
    'mixer',
    'capture',
    'outputs',
    'analogue',
    'meters',
    'presets',
    'ccmap',
    'monitor',
  ]) {
    $(`panel-${name}`).hidden = name !== next;
  }
  await refresh();
}

async function main() {
  bridge = await createBackend();
  // The mock has no audio path, so the tab only appears when there is one.
  $('tab-meters').hidden = bridge.kind !== 'device';

  $('tabs').onclick = (e) => {
    if (e.target.dataset.tab) selectTab(e.target.dataset.tab);
  };
  $('undo').onclick = async () => {
    await bridge.undo();
    await refresh();
  };
  $('redo').onclick = async () => {
    await bridge.redo();
    await refresh();
  };
  $('save').onclick = async () => {
    await bridge.save();
    await refresh();
  };
  $('clearlog').onclick = async () => {
    await bridge.clearMonitor();
    log = await bridge.monitor();
    renderMonitor();
  };
  $('slot').onclick = async () => {
    // Loading the slot before editing is the whole point: connecting over USB forces
    // hosted mode, so editing standalone without restoring it edits the wrong thing.
    const toStandalone = snap.status.editing !== 'standalone';
    await bridge.beginEdit(toStandalone);
    await refresh();
  };
  $('presetsavebtn').onclick = async () => {
    const field = $('presetname');
    try {
      await bridge.presetSave(field.value);
      field.value = '';
      showNotice('Saved.');
    } catch (e) {
      showNotice(`Save failed: ${e}`);
    }
    await refresh();
  };

  $('slot').title = 'Click to switch which flash slot you are editing (loads it first)';
  $('slot').style.cursor = 'pointer';

  // Drive the initial panel from the `tab` variable rather than trusting the markup to
  // agree with it. Two sources of truth for "which tab is showing" is how you get a
  // highlighted tab over the wrong panel.
  await selectTab(tab);

  if (bridge.kind === 'device') {
    // Meters get their own cadence: the audio stream updates far faster than a
    // configuration change, and re-rendering the whole UI at meter rate would be waste.
    setInterval(async () => {
      if (tab !== 'meters') return;
      meters = await bridge.meters();
      renderMeters();
    }, 50);

    // Follow the module. Two reasons this is not optional: the backend connects on a
    // background thread, so the first snapshot can predate the configuration dump; and
    // the ES-9's mixer is driven by MIDI CC from a controller, so the module's state
    // changes without this application doing anything. A UI that only updates when you
    // click it would quietly show a mix that is no longer the one playing.
    setInterval(async () => {
      if (interacting || pendingSends.size > 0) return;
      await refresh();
    }, 1000);
  }
}

main();
