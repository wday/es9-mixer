// A patchbay view of the module's routing.
//
// The routing is not a free matrix. Every capture channel holds exactly one source, and
// every block output holds exactly one destination; the other side of each connection
// fans out -- one source may feed many channels, and several mixes may land on the same
// output and sum there. A grid of cells states none of that. It reads as a field of
// independent switches, and the one-assignment-per-channel rule has to be written
// underneath it in prose. Two rows of jacks with a cable strung between them states it
// structurally: a channel jack holds one plug, and repatching visibly *moves* that plug
// rather than lighting a second cell.
//
// Everything here is presentation, rebuilt from the snapshot on every render. The only
// thing it keeps between renders is which jack is armed, so that the one-second
// background refresh cannot drop a half-made patch.

const SVG = 'http://www.w3.org/2000/svg';

const PAD = 14; // dead panel either side of the jack row
const JACK_R = 9; // bezel radius
const MIN_PITCH = 22; // below this the numbers stop fitting, so the bay scrolls instead
const MAX_PITCH = 34;
const BAND_H = 16; // the labelled bar naming each family of jacks
const MIN_GAP = 104; // vertical run available to the cables
const MAX_GAP = 210;
const CHROME = 88; // everything in a bay that is not the cable gap

// The jack half-way through a patch, as `${bayId}/${side}/${index}`, or null. Module
// scope on purpose: `render()` throws this DOM away and rebuilds it once a second.
let armed = null;
// The jack under the pointer. Not persisted -- a rebuild legitimately ends a hover.
let hover = null;

export function clearArmed() {
  armed = null;
}

// ---------------------------------------------------------------- label handling

// "Input 3" -> Input / 3. "S/PDIF L" -> S/PDIF / L. An undecoded wire byte reads as
// "? 0xAB" and has no numeric tail, so it falls through whole rather than being cut up.
function splitLabel(label) {
  const m = /^(.+?) ([0-9]+|[LR])$/.exec(label);
  return m ? { group: m[1], short: m[2] } : { group: label, short: label };
}

// Cable and jack colour follow the signal's family, not its position, so the same signal
// is the same colour in both bays and in every future view that draws one.
function family(group) {
  const g = group.toLowerCase();
  if (g.startsWith('input')) return 'input';
  if (g.startsWith('bus')) return 'bus';
  if (g.startsWith('usb')) return 'usb';
  if (g.startsWith('s/pdif')) return 'spdif';
  if (g.startsWith('mix')) return 'mix';
  if (g.startsWith('main') || g.startsWith('phones')) return 'main';
  if (g.startsWith('es-5')) return 'es5';
  if (g.startsWith('output')) return 'output';
  return 'other';
}

// Runs of adjacent jacks that share a family, for the bars above and below the rows.
// They replace the gaps a rack would use, because a gap in one row would break the
// vertical alignment that makes a straight-through patch look straight.
function bands(items) {
  const out = [];
  items.forEach((it, i) => {
    const last = out[out.length - 1];
    if (last && last.group === it.group && last.inert === it.inert) last.end = i;
    else out.push({ group: it.group, start: i, end: i, inert: it.inert });
  });
  for (const b of out) {
    const first = items[b.start].short;
    const last = items[b.end].short;
    const numeric = /^[0-9]+$/.test(first) && /^[0-9]+$/.test(last);
    b.text = b.end > b.start && numeric ? `${b.group} ${first}–${last}` : b.group;
  }
  return out;
}

// ---------------------------------------------------------------- cable geometry

// A cable leaves the top jack heading down, bellies out under its own weight, and rises
// into the bottom jack. The belly grows with the horizontal run -- a long patch needs
// more slack -- and is capped so it stays inside the gap instead of crossing the labels.
function cablePath(x1, y1, x2, y2) {
  const dy = y2 - y1;
  const run = Math.abs(x2 - x1);
  const s = Math.min(dy * 0.85, dy * 0.32 + run * 0.13);
  return `M ${x1} ${y1} C ${x1} ${y1 + s}, ${x2} ${y2 - s * 0.45}, ${x2} ${y2}`;
}

function svgEl(tag, attrs, parent) {
  const e = document.createElementNS(SVG, tag);
  for (const k of Object.keys(attrs)) {
    if (attrs[k] !== undefined && attrs[k] !== null) e.setAttribute(k, String(attrs[k]));
  }
  if (parent) parent.append(e);
  return e;
}

function titled(node, text) {
  const t = document.createElementNS(SVG, 'title');
  t.textContent = text;
  node.append(t);
  return node;
}

// ---------------------------------------------------------------- the bay

// `spec` describes one bay:
//   id            unique, used to key the armed jack
//   top, bottom   { title, kind: 'choice' | 'channel', items }
//                 a channel item carries { block, channel, wire, label, known }
//                 a choice  item carries { wire, label }
//   apply(channelItem, wire)   writes the connection; the caller refreshes
//   inert(item)   optional, true for jacks whose block is currently disabled
export function renderBay(host, spec) {
  host.innerHTML = '';

  const rows = {
    top: spec.top.items.map((it, i) => decorate(it, spec, 'top', i)),
    bottom: spec.bottom.items.map((it, i) => decorate(it, spec, 'bottom', i)),
  };
  const n = Math.max(rows.top.length, rows.bottom.length);

  const availW = host.clientWidth || 960;
  const availH = host.clientHeight || 320;
  const pitch = Math.min(MAX_PITCH, Math.max(MIN_PITCH, (availW - 2 * PAD) / n));
  const width = Math.round(2 * PAD + n * pitch);
  const gap = Math.round(Math.min(MAX_GAP, Math.max(MIN_GAP, availH - CHROME - 30)));

  const yTopNum = 2 + BAND_H + 12;
  const yTop = yTopNum + 3 + JACK_R;
  const yBot = yTop + gap;
  const yBotNum = yBot + JACK_R + 12;
  const height = yBotNum + 4 + BAND_H + 2;
  const x = (i) => Math.round(PAD + pitch * (i + 0.5));

  const wrap = document.createElement('div');
  wrap.className = 'bay-scroll';
  const svg = svgEl('svg', {
    class: 'bay',
    width,
    height,
    viewBox: `0 0 ${width} ${height}`,
  });
  wrap.append(svg);

  const gPanel = svgEl('g', { class: 'bay-panel' }, svg);
  const gCables = svgEl('g', { class: 'bay-cables' }, svg);
  const gJacks = svgEl('g', { class: 'bay-jacks' }, svg);

  // Panel: the rail each row of jacks is mounted on, and the family bars. A rail is only
  // as wide as its own row -- the source row is nearly twice the channel row, and a rail
  // spanning both reads as two dozen jacks missing rather than as a narrower panel.
  const railW = (side) => Math.round(2 * PAD + rows[side].length * pitch);
  svgEl(
    'rect',
    { class: 'rail', x: 0, y: 0, width: railW('top'), height: yTop + JACK_R + 6, rx: 6 },
    gPanel,
  );
  svgEl(
    'rect',
    {
      class: 'rail',
      x: 0,
      y: yBot - JACK_R - 6,
      width: railW('bottom'),
      height: height - (yBot - JACK_R - 6),
      rx: 6,
    },
    gPanel,
  );

  for (const [side, y] of [
    ['top', 2],
    ['bottom', yBotNum + 4],
  ]) {
    for (const b of bands(rows[side])) {
      const x0 = x(b.start) - pitch / 2 + 1;
      const w = pitch * (b.end - b.start + 1) - 2;
      const cls = `band fam-${family(b.group)}${b.inert ? ' inert' : ''}`;
      svgEl('rect', { class: cls, x: x0, y, width: w, height: BAND_H, rx: 4 }, gPanel);
      const label = svgEl(
        'text',
        { class: 'band-text', x: x0 + w / 2, y: y + BAND_H / 2 + 3.5 },
        gPanel,
      );
      label.textContent = fitText(b.text, w);
      titled(label, b.text);
    }
  }

  // Jacks.
  const jackEls = { top: [], bottom: [] };
  for (const side of ['top', 'bottom']) {
    const y = side === 'top' ? yTop : yBot;
    const yNum = side === 'top' ? yTopNum : yBotNum;
    rows[side].forEach((it, i) => {
      const g = svgEl(
        'g',
        {
          class: `jack fam-${it.fam}${it.inert ? ' inert' : ''}`,
          tabindex: 0,
          role: 'button',
          'data-side': side,
          'data-i': i,
        },
        gJacks,
      );
      svgEl('circle', { class: 'bezel', cx: x(i), cy: y, r: JACK_R }, g);
      svgEl('circle', { class: 'hole', cx: x(i), cy: y, r: JACK_R - 4.4 }, g);
      const num = svgEl('text', { class: 'jack-num', x: x(i), y: yNum + (side === 'top' ? 0 : 0) }, g);
      num.textContent = it.short;
      titled(g, it.tip);
      jackEls[side].push({ g, item: it, x: x(i), y });
    });
  }

  // Cables. One per channel jack, because that is the side that holds exactly one plug.
  const cables = [];
  const chanSide = spec.channelSide;
  const otherSide = chanSide === 'top' ? 'bottom' : 'top';
  rows[chanSide].forEach((chan, ci) => {
    const oi = rows[otherSide].findIndex((o) => o.wire === chan.wire);
    const from = chanSide === 'top' ? { side: 'top', i: ci } : { side: 'top', i: oi };
    const to = chanSide === 'top' ? { side: 'bottom', i: oi } : { side: 'bottom', i: ci };
    const topIt = rows.top[from.i];
    const botIt = rows.bottom[to.i];

    if (oi < 0) {
      // An undecoded wire byte has no jack to land on. Draw the stub rather than
      // dropping the connection silently -- the value is real and round-trips.
      const j = jackEls[chanSide][ci];
      const dir = chanSide === 'top' ? 1 : -1;
      const d = `M ${j.x} ${j.y + dir * (JACK_R - 1)} l 0 ${dir * 26}`;
      const g = svgEl('g', { class: 'cable orphan' }, gCables);
      svgEl('path', { class: 'casing', d, fill: 'none' }, g);
      titled(g, `${chan.label} — unrecognised routing value, left untouched`);
      cables.push({ g, fam: 'other', top: chanSide === 'top' ? ci : -1, bottom: chanSide === 'top' ? -1 : ci });
      return;
    }

    // Colour follows the signal, which in both bays is whatever the top row holds.
    const g = svgEl('g', { class: `cable fam-${topIt.fam}` }, gCables);
    const d = cablePath(x(from.i), yTop + JACK_R - 1, x(to.i), yBot - JACK_R + 1);
    svgEl('path', { class: 'hit', d, fill: 'none' }, g);
    svgEl('path', { class: 'casing', d, fill: 'none' }, g);
    svgEl('path', { class: 'core', d, fill: 'none' }, g);
    titled(g, `${topIt.label} → ${botIt.label}`);
    cables.push({ g, fam: topIt.fam, top: from.i, bottom: to.i });
  });

  // Plugs sit on top of the cable ends, so a cable reads as going *into* the jack rather
  // than stopping at it. A plug takes the colour of its cable, not of the jack it is in:
  // on the channel row those differ, and it is the arriving signal that is worth naming.
  // Where several cables land on one destination and the module sums them, no single
  // family is the truth, so the plug goes neutral rather than picking one arbitrarily.
  for (const side of ['top', 'bottom']) {
    jackEls[side].forEach((j, i) => {
      const landed = cables.filter((c) => c[side] === i);
      if (!landed.length) return;
      const fam = landed.every((c) => c.fam === landed[0].fam) ? landed[0].fam : 'other';
      j.g.classList.add('patched');
      svgEl('circle', { class: `plug fam-${fam}`, cx: j.x, cy: j.y, r: JACK_R - 3.2 }, j.g);
      if (landed.length === 1) return;

      // More than one cable on a jack means something the grid never showed. On the
      // source side it is fan-out: one signal feeding several channels. On the
      // destination side it is the module summing them -- which is how both mixer banks
      // reach the main outs, and is easy to create by accident. Either way the jack is
      // shared, so it gets a ring and its tooltip names everything on it.
      const far = side === 'top' ? 'bottom' : 'top';
      const names = landed.map((c) => rows[far][c[far]]?.label).filter(Boolean);
      j.g.classList.add('shared');
      svgEl('circle', { class: 'shared-ring', cx: j.x, cy: j.y, r: JACK_R + 2.6, fill: 'none' }, j.g);
      j.g.querySelector('title').textContent =
        `${j.item.tip} — ${names.length} connections: ${names.join(', ')}`;
    });
  }

  const readout = document.createElement('div');
  readout.className = 'bay-readout';

  host.append(wrap, readout);

  // ------------------------------------------------------------ state painting

  const key = (side, i) => `${spec.id}/${side}/${i}`;
  const parse = (k) => {
    const [id, side, i] = k.split('/');
    return id === spec.id ? { side, i: Number(i) } : null;
  };

  function touches(cable, sel) {
    return cable[sel.side] === sel.i;
  }

  function paint() {
    const sel = armed ? parse(armed) : null;
    const hov = !sel && hover ? parse(hover) : null;
    const focus = sel || hov;

    svg.classList.toggle('focused', !!focus);
    svg.classList.toggle('arming', !!sel);

    for (const c of cables) c.g.classList.toggle('hot', !!focus && touches(c, focus));
    for (const side of ['top', 'bottom']) {
      jackEls[side].forEach((j, i) => {
        const isFocus = !!focus && focus.side === side && focus.i === i;
        const linked = !!focus && cables.some((c) => touches(c, focus) && c[side] === i);
        j.g.classList.toggle('armed', !!sel && sel.side === side && sel.i === i);
        j.g.classList.toggle('hot', isFocus || linked);
        // While a patch is half made, the far row is the only place it can land.
        j.g.classList.toggle('target', !!sel && sel.side !== side);
      });
    }

    if (sel) {
      const it = rows[sel.side][sel.i];
      const dest = sel.side === chanSide ? spec[otherSide].title : spec[chanSide].title;
      readout.className = 'bay-readout live';
      readout.textContent = `Patching ${it.label} — click a jack in ${dest.toLowerCase()} to land it, or press Escape.`;
    } else if (hov) {
      const it = rows[hov.side][hov.i];
      const linked = cables
        .filter((c) => touches(c, hov))
        .map((c) => rows[hov.side === 'top' ? 'bottom' : 'top'][c[hov.side === 'top' ? 'bottom' : 'top']])
        .filter(Boolean)
        .map((o) => o.label);
      readout.className = 'bay-readout';
      readout.textContent = linked.length
        ? `${it.label} — ${hov.side === 'top' ? '→' : '←'} ${linked.join(', ')}`
        : `${it.label} — not patched`;
    } else {
      readout.className = 'bay-readout';
      readout.textContent = spec.idle;
    }
  }

  // ------------------------------------------------------------ patching

  async function activate(side, i) {
    const here = key(side, i);
    if (armed === here) {
      armed = null;
      paint();
      return;
    }
    const sel = armed ? parse(armed) : null;
    if (!sel || sel.side === side) {
      armed = here;
      paint();
      return;
    }
    const chan = rows[chanSide][side === chanSide ? i : sel.i];
    const choice = rows[otherSide][side === chanSide ? sel.i : i];
    armed = null;
    if (chan.wire === choice.wire) {
      paint();
      return;
    }
    await spec.apply(chan, choice.wire);
  }

  svg.addEventListener('pointerover', (e) => {
    const g = e.target.closest?.('.jack');
    hover = g ? key(g.dataset.side, Number(g.dataset.i)) : hover;
    if (g) paint();
  });
  svg.addEventListener('pointerout', (e) => {
    if (e.target.closest?.('.jack') && !svg.contains(e.relatedTarget)) {
      hover = null;
      paint();
    }
  });
  wrap.addEventListener('pointerleave', () => {
    hover = null;
    paint();
  });
  svg.addEventListener('click', (e) => {
    const g = e.target.closest?.('.jack');
    if (g) activate(g.dataset.side, Number(g.dataset.i));
  });
  svg.addEventListener('keydown', (e) => {
    const g = e.target.closest?.('.jack');
    if (g && (e.key === 'Enter' || e.key === ' ')) {
      e.preventDefault();
      activate(g.dataset.side, Number(g.dataset.i));
    }
  });

  paint();
}

// Decorates a raw view item with everything the drawing needs.
function decorate(it, spec, side, i) {
  const { group, short } = splitLabel(it.label);
  const inert = spec.inert ? spec.inert(it, side) : false;
  return {
    ...it,
    group: it.group ?? group,
    short: it.short ?? short,
    fam: family(it.group ?? group),
    inert,
    tip: it.tip ?? it.label,
  };
}

// The family bars carry a real name, not an abbreviation, whenever it fits; when it does
// not the full text stays reachable as a tooltip rather than being silently truncated.
function fitText(text, w) {
  const max = Math.floor((w - 6) / 5.6);
  if (text.length <= max) return text;
  if (max < 3) return '';
  return `${text.slice(0, max - 1)}…`;
}
