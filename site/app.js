// spoilheap landing: a trench section drawn from real `spoilheap --json` output.
// Strata = agent leftovers (thickness to scale), "natural" at the bottom = free space.
(() => {
'use strict';
const $ = (s) => document.querySelector(s);
const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
const GB = 1e9;
const fmt = (b) => (b >= 99.95 * GB ? (b / GB).toFixed(0) : (b / GB).toFixed(1)) + ' GB';

// soil: base colour, stipple tints, pebble density (per 10k px²), pebble size, dot size
const SOIL = {
  humus:  { base: '#3a2b22', dots: ['#2a1f19', '#4b392d', '#5d544b'], peb: 0.5, ps: [2, 5], ds: [0.5, 1.2], roots: true },
  loam:   { base: '#6e4f37', dots: ['#5b412d', '#85624a', '#4a3627'], peb: 1.4, ps: [2, 6], ds: [0.5, 1.3] },
  clay:   { base: '#9c5b3b', dots: ['#8b5033', '#ad6b4a', '#7d4a31'], peb: 0.35, ps: [2, 4], ds: [0.4, 0.9] },
  sand:   { base: '#cfa962', dots: ['#b9934f', '#dfbd7d', '#a88a55'], peb: 0.6, ps: [1.5, 3.5], ds: [0.7, 1.6] },
  gravel: { base: '#8c8679', dots: ['#77726a', '#a39d91', '#6a645b'], peb: 7, ps: [2.5, 8], ds: [0.6, 1.4] },
  natural:{ base: '#f4f2ee', dots: ['#dcd8d0', '#c9c4ba', '#e6e3dc', '#b9ab97'], peb: 0.9, ps: [3, 9], ds: [0.6, 1.8], dense: 2.2, flint: true, bed: true },
};
const PEBBLES = ['#b9ab97', '#5d544b', '#6e4f37', '#8c8679'];
const VCOL = { SAFE: '#a6d13c', REVIEW: '#f07a1a', KEEP: '#2f6db5' };

// newest on top; ids are the context numbers spoilheap printed for the demo home
const STRATA = [
  { ids: [103, 115, 117, 121, 106], v: 'KEEP', soil: 'humus', name: 'recent history, live work' },
  { ids: [102], v: 'SAFE', soil: 'sand', name: 'Codex crash dumps' },
  { ids: [107], v: 'SAFE', soil: 'loam', name: 'target/ in a dirty worktree' },
  { ids: [127, 128], v: 'SAFE', soil: 'gravel', name: 'half-written git packs' },
  { ids: [105, 112, 116, 118, 123, 109, 111], v: 'SAFE', soil: 'clay', name: 'logs, node_modules, merged worktrees' },
  { ids: [110], v: 'SAFE', soil: 'sand', name: 'target/ in a merged worktree' },
  { ids: [104, 119, 120, 122, 124, 125, 126, 114, 108], v: 'REVIEW', soil: 'loam', name: 'archives, undo snapshots, old chats' },
  { ids: [113], v: 'REVIEW', soil: 'gravel', name: 'Claude transcripts > 30 d' },
  { ids: [101], v: 'REVIEW', soil: 'clay', name: 'Codex sessions > 30 d' },
];

// ---------- small helpers ----------
function rng(seed) { let a = seed >>> 0; return () => { a |= 0; a = (a + 0x6d2b79f5) | 0; let t = Math.imul(a ^ (a >>> 15), 1 | a); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }; }
function noise1(seed) {
  const r = rng(seed), oct = [[170, 1], [64, 0.45], [23, 0.18]].map(([s, a]) => ({ s, a, v: Array.from({ length: 64 }, r) }));
  return (x) => { let y = 0; for (const o of oct) { const p = x / o.s, i = Math.floor(p), f = p - i, u = f * f * (3 - 2 * f); y += o.a * ((o.v[i & 63] * (1 - u) + o.v[(i + 1) & 63] * u) * 2 - 1); } return y / 1.63; };
}
const ease = (t) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);

const dig = $('#dig'), cv = $('#wall'), g = cv.getContext('2d');
const board = $('#board'), gauge = $('#gauge'), tagsEl = $('#tags'), sheet = $('#sheet');
const freeEl = $('#free'), floorMsg = $('#floormsg'), btn = $('#shovel');
let items = {}, layers = [], natural = null, heap = null, grains = [], turf = [];
let W = 0, H = 0, dpr = 1, narrow = false, groundY = 0, wallBot = 0, ppb = 0, rodX = 26, rodW = 14;
let freeBase = 3.1 * GB, freeNow = freeBase, floor = 10 * GB, total = 0;
let state = 'intro', scrape = reduce ? 1 : 0, raf = 0, t0 = 0, plan = [], autoplayed = false;

// ---------- textures ----------
function texture(soil, w, h, seed) {
  const c = document.createElement('canvas');
  c.width = Math.max(1, Math.ceil(w * dpr)); c.height = Math.max(1, Math.ceil(h * dpr));
  const x = c.getContext('2d'), s = SOIL[soil], r = rng(seed);
  x.scale(dpr, dpr);
  x.fillStyle = s.base; x.fillRect(0, 0, w, h);
  const n = Math.round((w * h) / 34 * (s.dense || 1));
  for (let i = 0; i < n; i++) {
    x.fillStyle = s.dots[(r() * s.dots.length) | 0];
    x.globalAlpha = 0.45 + r() * 0.5;
    const d = s.ds[0] + r() * (s.ds[1] - s.ds[0]);
    x.fillRect(r() * w, r() * h, d, d * (0.7 + r() * 0.6));
  }
  x.globalAlpha = 1;
  if (s.roots) {
    x.strokeStyle = '#1f1b18'; x.globalAlpha = 0.5; x.lineWidth = 0.8;
    for (let i = 0; i < w / 60; i++) {
      let px = r() * w, py = r() * h * 0.5; x.beginPath(); x.moveTo(px, py);
      for (let k = 0; k < 5; k++) { px += (r() - 0.5) * 10; py += 3 + r() * 6; x.lineTo(px, py); }
      x.stroke();
    }
    x.globalAlpha = 1;
  }
  if (s.bed) { // faint bedding planes in the chalk
    x.strokeStyle = 'rgba(140,134,121,.35)'; x.lineWidth = 1;
    for (let y = 18 + r() * 20; y < h; y += 26 + r() * 30) { x.beginPath(); x.moveTo(0, y); for (let px = 0; px <= w; px += 40) x.lineTo(px, y + (r() - 0.5) * 3); x.stroke(); }
  }
  // pebbles: dart-throwing Poisson disk, two-tone irregular polygons
  const want = Math.round((w * h) / 10000 * s.peb), pts = [];
  for (let tries = 0; tries < want * 12 && pts.length < want; tries++) {
    const p = { x: r() * w, y: r() * h, rad: s.ps[0] + Math.pow(r(), 2.2) * (s.ps[1] - s.ps[0]) };
    if (pts.every((q) => (q.x - p.x) ** 2 + (q.y - p.y) ** 2 > (q.rad + p.rad + 3) ** 2)) pts.push(p);
  }
  for (const p of pts) {
    const k = 6 + ((r() * 3) | 0), rot = r() * 6.3, sq = 0.55 + r() * 0.35, poly = [];
    for (let i = 0; i < k; i++) { const a = rot + (i / k) * 6.283, rr = p.rad * (0.75 + r() * 0.35); poly.push([p.x + Math.cos(a) * rr, p.y + Math.sin(a) * rr * sq]); }
    x.fillStyle = s.flint ? (r() < 0.7 ? '#3a2b22' : '#5d544b') : PEBBLES[(r() * PEBBLES.length) | 0];
    if (x.fillStyle === s.base) x.fillStyle = '#5d544b';
    x.beginPath(); poly.forEach(([a, b], i) => (i ? x.lineTo(a, b) : x.moveTo(a, b))); x.closePath(); x.fill();
    x.fillStyle = 'rgba(244,242,238,.28)';
    x.beginPath(); poly.forEach(([a, b], i) => { const ax = p.x + (a - p.x) * 0.55 - p.rad * 0.18, by = p.y + (b - p.y) * 0.5 - p.rad * 0.16; i ? x.lineTo(ax, by) : x.moveTo(ax, by); }); x.closePath(); x.fill();
    x.strokeStyle = 'rgba(21,20,19,.35)'; x.lineWidth = 0.6;
    x.beginPath(); poly.forEach(([a, b], i) => (i ? x.lineTo(a, b) : x.moveTo(a, b))); x.closePath(); x.stroke();
  }
  return c;
}

// ---------- layout ----------
function build() {
  dpr = Math.min(2, window.devicePixelRatio || 1);
  W = dig.clientWidth; narrow = W < 900;
  rodX = narrow ? 8 : 26; rodW = narrow ? 10 : 14;
  if (narrow) {
    board.style.left = '16px'; board.style.width = ''; board.style.top = gauge.offsetTop + gauge.offsetHeight + 18 + 'px';
    groundY = board.offsetTop + board.offsetHeight + 130;
    dig.style.minHeight = groundY + Math.min(720, Math.max(560, Math.round(window.innerHeight * 0.75))) + 'px';
  } else {
    dig.style.minHeight = '';
    groundY = Math.round(Math.max(196, dig.clientHeight * 0.22));
    board.style.top = groundY + 40 + 'px';
    board.style.left = rodX + rodW + 52 + 'px';
    board.style.width = Math.min(590, W * 0.47) + 'px';
  }
  H = dig.clientHeight; wallBot = H;
  cv.width = Math.round(W * dpr); cv.height = Math.round(H * dpr);
  const done = state === 'done';
  total = Object.values(items).reduce((a, i) => a + i.b, 0) + freeBase;
  ppb = (wallBot - groundY) / total;
  layers = STRATA.map((d, i) => {
    const its = d.ids.map((id) => items[id]).filter(Boolean);
    const bytes = its.reduce((a, it) => a + it.b, 0);
    const t0 = bytes * ppb, pad = 12;
    return { ...d, its, bytes, t0, t: d.v === 'SAFE' && done ? 0 : t0, pad, tex: texture(d.soil, W, t0 + pad * 2, 101 + i * 7), wave: noise1(900 + i * 31), amp: Math.min(13, 3 + t0 / 7) };
  });
  const removed = done ? layers.filter((l) => l.v === 'SAFE').reduce((a, l) => a + l.bytes, 0) : 0;
  freeNow = freeBase + removed;
  natural = { soil: 'natural', name: 'natural · free space', tex: texture('natural', W, (wallBot - groundY) + 24, 77), wave: noise1(4242), amp: 4, pad: 12 };
  const tr = rng(5);
  turf = Array.from({ length: Math.ceil(W / 5) }, (_, i) => ({ x: i * 5 + tr() * 4, h: 2 + tr() * 6, lean: (tr() - 0.5) * 4 }));
  buildHeap(removed);
  buildTags();
  draw();
}

function buildHeap(removed) {
  const cw = narrow ? 3 : 4, ch = cw * 0.675; // 34° angle of repose
  const x0 = narrow ? W * 0.4 : W * 0.6, x1 = W - (narrow ? 10 : 24);
  const top = narrow ? board.offsetTop + board.offsetHeight + 12 : 64;
  const cols = Math.floor((x1 - x0) / cw), maxH = Math.floor((groundY - top) / ch);
  heap = { cw, ch, x0, cols, maxH, h: new Int16Array(cols), st: Array.from({ length: cols }, () => []), q: [] };
  const cap = Math.min(cols / 2, maxH) ** 2;
  heap.budget = Math.max(300, Math.min(narrow ? 1100 : 2300, Math.floor(cap * 0.66)));
  const r = rng(11), old = ['#4b392d', '#5b412d', '#3a2b22', '#6e4f37', '#85624a'];
  const pre = Math.round(heap.budget * 0.06);
  for (let i = 0; i < pre; i++) drop(Math.round(cols * 0.62 + (r() - 0.5) * 6), old[(r() * old.length) | 0], true);
  if (removed) { // reload in the done state: rebuild the full heap at once
    pileAtOnce(layers.filter((l) => l.v === 'SAFE'), removed, r);
  }
}
// the heap the animated dig leaves behind, built in one go: shovelfuls land on the same
// schedule as in shovel() (each stratum starts before the last one is done), grains coloured alike
function pileAtOnce(safe, sum, r) {
  const g = []; let at = 0;
  for (const l of safe) {
    const s = SOIL[l.soil], share = l.bytes / sum, D = 650 + 1700 * share, n = Math.round(heap.budget * share);
    for (let i = 0; i < n; i++) g.push({ t: at + (i / n) * D + 700 * r(), c: r() < 0.25 ? s.base : s.dots[(r() * 3) | 0] });
    at += D * 0.6;
  }
  g.sort((a, b) => a.t - b.t);
  for (const p of g) drop(aim(r), p.c, true);
}
let dumpAt = 0.55;
function aim(r) { dumpAt += (r() - 0.5) * 0.02; dumpAt = Math.max(0.42, Math.min(0.66, dumpAt)); const u = (r() + r() + r() - 1.5) / 1.5; return Math.round(heap.cols * dumpAt + u * heap.cols * 0.1); }
function drop(c, col, instant) {
  const hp = heap; c = Math.max(0, Math.min(hp.cols - 1, c));
  hp.h[c]++; hp.st[c].push(col);
  if (instant) { let k = c; for (let guard = 0; guard < 400; guard++) { const n = slide(k); if (n < 0) break; k = n; } }
  else hp.q.push(c);
}
function slide(c) { // move the top grain downhill if the slope is steeper than the angle of repose
  const hp = heap, l = c > 0 ? hp.h[c] - hp.h[c - 1] : 0, rr = c < hp.cols - 1 ? hp.h[c] - hp.h[c + 1] : 0;
  let to = -1;
  const need = Math.random() < 0.18 ? 3 : 2;
  if (l >= need && rr >= need) to = Math.random() < 0.5 ? c - 1 : c + 1; else if (l >= need && c > 0) to = c - 1; else if (rr >= need && c < hp.cols - 1) to = c + 1;
  if (to < 0) return -1;
  hp.h[c]--; hp.h[to]++; hp.st[to].push(hp.st[c].pop());
  return to;
}

function buildTags() {
  tagsEl.innerHTML = '';
  const all = [...layers, natural];
  all.forEach((l, i) => {
    const li = document.createElement('li'); li.className = 'tag' + (scrape < 1 ? ' pre' : ' in');
    if (l === natural) {
      li.innerHTML = `<button type="button" aria-label="Natural: free space"><span class="no">NAT</span><span class="lbl">natural · free space</span><span class="gb" id="natgb">${fmt(freeNow)}</span></button>`;
    } else {
      const no = l.ids.length <= 2 ? l.ids.join(' ') : `${l.ids[0]} +${l.ids.length - 1}`;
      li.style.setProperty('--c', VCOL[l.v]);
      li.innerHTML = `<button type="button" aria-haspopup="dialog" aria-label="${l.v}: ${l.name}, ${fmt(l.bytes)}. Show the ${l.its.length} items"><span class="no">${no}</span><i class="fl" aria-hidden="true"></i><span class="vb">${l.v}</span><span class="lbl">${l.name}</span><span class="gb">${fmt(l.bytes)}</span></button>`;
      li.querySelector('button').addEventListener('click', (e) => openSheet(l, li, e));
      if (state === 'done' && l.v === 'SAFE') li.classList.add('gone');
    }
    if (scrape < 1) li.style.opacity = 0;
    l.el = li; tagsEl.appendChild(li);
  });
  placeTags();
}

function positions() { // top/bottom of every layer from the ground down; natural fills the rest
  let y = groundY;
  for (const l of layers) { l.top = y; y += l.t; l.bot = y; }
  natural.top = y; natural.bot = wallBot; natural.t = wallBot - y;
}
function placeTags() {
  positions();
  const gap = narrow ? 27 : 30, all = [...layers, natural].filter((l) => !(l.el.classList.contains('gone')));
  const want = all.map((l) => ({ l, y: Math.max(l.top, Math.min(l.bot, (l.top + l.bot) / 2)) - 13 }));
  for (let i = 1; i < want.length; i++) want[i].y = Math.max(want[i].y, want[i - 1].y + gap);
  const lim = wallBot - 40;
  for (let i = want.length - 1; i >= 0; i--) { if (want[i].y > lim) want[i].y = lim; lim2(i); }
  function lim2(i) { if (i < want.length - 1) want[i].y = Math.min(want[i].y, want[i + 1].y - gap); }
  for (const w of want) { w.l.el.style.top = w.y + 'px'; w.l.tagY = w.y + 13; }
}

// ---------- drawing ----------
function wavePath(l, y, x0, x1, dir) {
  const step = 8;
  if (dir > 0) for (let x = x0; x <= x1 + step; x += step) g.lineTo(x, y + l.wave(x) * l.amp);
  else for (let x = x1 + step; x >= x0 - step; x -= step) g.lineTo(x, y + l.wave(x) * l.amp);
}
function drawLayer(l, below) {
  if (l.t <= 0.3) return;
  const top = l.top, bot = l.bot;
  g.save(); g.beginPath(); g.moveTo(-10, top + l.wave(-10) * l.amp);
  wavePath(l, top, -10, W + 10, 1);
  if (below) wavePath(below, bot, -10, W + 10, -1); else { g.lineTo(W + 10, bot + 2); g.lineTo(-10, bot + 2); }
  g.closePath(); g.clip();
  const sy = l === natural ? top - l.pad : top - l.pad - (l.t0 - l.t) * 0.5;
  g.drawImage(l.tex, 0, sy, W, l.tex.height / dpr);
  g.restore();
  g.strokeStyle = 'rgba(21,20,19,.5)'; g.lineWidth = 1.1;
  g.beginPath(); g.moveTo(-10, top + l.wave(-10) * l.amp); wavePath(l, top, -10, W + 10, 1); g.stroke();
}
function draw(now = performance.now()) {
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.fillStyle = '#1f1b18'; g.fillRect(0, 0, W, H);
  positions();
  const reveal = groundY + scrape * (wallBot - groundY);
  g.save(); g.beginPath(); g.rect(0, 0, W, reveal + 8); g.clip();
  const all = [...layers, natural];
  for (let i = all.length - 1; i >= 0; i--) drawLayer(all[i], all[i + 1]);
  // datum line: the guard floor, 10 GB of natural above the bottom
  const dy = wallBot - floor * ppb;
  g.setLineDash([9, 6]); g.strokeStyle = freeNow >= floor ? '#a6d13c' : '#f4f2ee'; g.lineWidth = 2;
  g.beginPath(); g.moveTo(rodX + rodW + 4, dy); g.lineTo(W, dy); g.stroke(); g.setLineDash([]);
  g.fillStyle = '#151413'; g.fillRect(rodX + rodW + 6, dy - 22, 132, 18);
  g.fillStyle = '#f4f2ee'; g.font = '600 12px "Barlow Condensed", sans-serif'; g.textBaseline = 'middle';
  g.fillText('DATUM · GUARD FLOOR 10 GB', rodX + rodW + 12, dy - 13);
  g.restore();
  // turf along the trench edge
  g.strokeStyle = '#3a2b22'; g.lineWidth = 1.4;
  g.beginPath(); for (const t of turf) { g.moveTo(t.x, groundY + 1); g.lineTo(t.x + t.lean, groundY - t.h); } g.stroke();
  // spoil heap
  const hp = heap;
  for (let c = 0; c < hp.cols; c++) {
    const st = hp.st[c], x = hp.x0 + c * hp.cw;
    for (let k = 0; k < st.length; k++) { const j = ((c * 7919 + k * 104729) % 13) / 13; g.fillStyle = st[k]; g.fillRect(x + (j - 0.5) * 1.6, groundY - (k + 1) * hp.ch - j, hp.cw + 0.8, hp.ch + 0.8); }
  }
  // ranging rod: 10 GB bands from the surface down
  const band = 10 * GB * ppb;
  g.fillStyle = '#151413'; g.fillRect(rodX - 2, groundY - 34, rodW + 4, wallBot - groundY + 34);
  for (let y = groundY - 32, i = 0; y < wallBot; y += band, i++) { g.fillStyle = i % 2 ? '#f4f2ee' : '#d0312d'; g.fillRect(rodX, y, rodW, Math.min(band, wallBot - y) - 1); }
  g.save(); g.translate(rodX + rodW / 2 + 0.5, groundY - 40); g.fillStyle = '#c9cdd1'; g.font = '600 11px "Barlow Condensed", sans-serif'; g.textAlign = 'center'; g.fillText('10 GB', 0, -2); g.restore();
  // nails from each tag to its layer
  g.strokeStyle = 'rgba(201,205,209,.9)'; g.lineWidth = 1.2;
  for (const l of all) {
    if (!l.el || l.el.classList.contains('gone') || l.el.style.opacity === '0') continue;
    const b = l.el.firstElementChild, tx = W - (narrow ? 10 : Math.min(32, Math.max(10, W * 0.02))) - b.offsetWidth;
    const ly = Math.max(l.top + 3, Math.min(l.bot - 3, (l.top + l.bot) / 2));
    if (l.tagY > reveal) continue;
    g.beginPath(); g.moveTo(tx, l.tagY); g.lineTo(tx - 16, ly); g.stroke();
    g.fillStyle = '#c9cdd1'; g.beginPath(); g.arc(tx - 16, ly, 2.4, 0, 6.3); g.fill();
  }
  // grains in flight
  for (const p of grains) {
    const u = Math.min(1, (now - p.t0) / p.T); if (u < 0) continue;
    const a = 1 - u, x = a * a * p.x0 + 2 * a * u * p.cx + u * u * p.x1, y = a * a * p.y0 + 2 * a * u * p.cy + u * u * p.y1;
    g.fillStyle = p.c; g.fillRect(x - 1.5, y - 1.5, 3, 3);
  }
  // the scraper during the intro
  if (scrape < 1) {
    g.fillStyle = '#c9cdd1'; g.fillRect(0, reveal - 1, W, 3);
    g.fillStyle = '#151413'; g.fillRect(0, reveal + 2, W, 1);
  }
}

// ---------- the dig ----------
function setFree(b) {
  freeNow = b; freeEl.textContent = fmt(b);
  const nat = document.getElementById('natgb'); if (nat) nat.textContent = fmt(b);
  const ok = b >= floor; dig.classList.toggle('ok', ok);
  floorMsg.textContent = ok ? 'above the 10 GB guard floor: agents can keep working' : "under the 10 GB guard floor: the agent's next tool call is blocked";
}
function intro() {
  if (reduce) { scrape = 1; if (state === 'intro') state = 'idle'; [...layers, natural].forEach((l) => { l.el.style.opacity = ''; l.el.classList.remove('pre'); l.el.classList.add('in'); }); placeTags(); draw(); return; }
  const start = performance.now(), D = 1500;
  const step = (now) => {
    scrape = Math.min(1, ease((now - start) / D));
    const reveal = groundY + scrape * (wallBot - groundY);
    for (const l of [...layers, natural]) if (l.el.style.opacity === '0' && l.tagY <= reveal + 4) { l.el.style.opacity = ''; l.el.classList.add('in'); }
    draw(now);
    if (scrape < 1) requestAnimationFrame(step); else { state = 'idle'; maybeAutoplay(); }
  };
  requestAnimationFrame(step);
}
function maybeAutoplay() {
  if (autoplayed || reduce) return;
  autoplayed = true;
  setTimeout(() => { if (state === 'idle' && window.scrollY < 120 && !document.hidden && !sheet.contains(document.activeElement)) shovel(); }, 1400);
}
function shovel() {
  if (state === 'digging') return;
  if (state === 'done') return backfill();
  closeSheet();
  const safe = layers.filter((l) => l.v === 'SAFE'), sum = safe.reduce((a, l) => a + l.bytes, 0);
  layers.filter((l) => l.v !== 'SAFE').forEach((l) => { l.el.classList.remove('wiggle'); void l.el.offsetWidth; l.el.classList.add('wiggle'); });
  if (reduce) {
    const r = rng(3);
    safe.forEach((l) => { l.t = 0; l.el.classList.add('gone'); });
    pileAtOnce(safe, sum, r);
    setFree(freeBase + sum); finish(); return;
  }
  state = 'digging'; btn.disabled = true; btn.textContent = 'Shovelling…';
  const r = rng(Date.now() & 0xffff), now = performance.now();
  let at = now + 150; plan = [];
  for (const l of safe) {
    const share = l.bytes / sum, D = 650 + 1700 * share, n = Math.max(12, Math.round(heap.budget * share));
    plan.push({ l, s: at, D, n, sent: 0, per: l.bytes / n });
    at += D * 0.6;
  }
  const flyTop = narrow ? board.offsetTop + board.offsetHeight + 8 : 70;
  let landed = 0;
  const step = (t) => {
    for (const p of plan) {
      const u = Math.max(0, Math.min(1, (t - p.s) / p.D));
      p.l.t = p.l.t0 * (1 - ease(u));
      if (u > 0.85) p.l.el.classList.add('gone');
      const due = Math.round(p.n * u);
      for (; p.sent < due; p.sent++) {
        const c = aim(r), x1 = heap.x0 + c * heap.cw + heap.cw / 2, y1 = groundY - heap.h[Math.max(0, Math.min(heap.cols - 1, c))] * heap.ch;
        const x0 = rodX + rodW + 30 + r() * (W - rodX - 60), y0 = p.l.top + r() * Math.max(2, p.l.t);
        const apex = Math.max(flyTop, Math.min(y0, y1) - 90 - r() * 120);
        grains.push({ x0, y0, x1, y1, cx: (x0 + x1) / 2 + (r() - 0.5) * 80, cy: apex, t0: t, T: 520 + r() * 520, c: r() < 0.25 ? SOIL[p.l.soil].base : SOIL[p.l.soil].dots[(r() * 3) | 0], col: c, b: p.per });
      }
    }
    grains = grains.filter((p) => { if (t - p.t0 >= p.T) { drop(p.col, p.c, true); landed += p.b; return false; } return true; });
    for (let k = 0; k < 60 && heap.q.length; k++) { const c = heap.q.shift(), n = slide(c); if (n >= 0) heap.q.push(n, c); }
    setFree(freeBase + landed);
    placeTags(); draw(t);
    const busy = grains.length || heap.q.length || plan.some((p) => p.sent < p.n || t < p.s + p.D);
    if (busy) raf = requestAnimationFrame(step);
    else { setFree(freeBase + sum); finish(); }
  };
  raf = requestAnimationFrame(step);
}
function finish() {
  state = 'done'; btn.disabled = false; btn.textContent = 'Backfill and dig again';
  layers.filter((l) => l.v === 'SAFE').forEach((l) => l.el.classList.add('gone'));
  placeTags(); draw();
}
function backfill() {
  cancelAnimationFrame(raf); grains = []; state = 'intro'; scrape = reduce ? 1 : 0; freeNow = freeBase; setFree(freeBase);
  btn.textContent = 'Shovel out SAFE · ' + fmt(layers.filter((l) => l.v === 'SAFE').reduce((a, l) => a + l.bytes, 0));
  build(); intro();
}

// ---------- context sheet ----------
let opener = null;
function openSheet(l, li) {
  opener = li.querySelector('button');
  $('#sheet-h').textContent = `${l.v} · ${l.name} · ${fmt(l.bytes)}`;
  sheet.style.setProperty('--c', VCOL[l.v]);
  $('#sheet-list').innerHTML = l.its.map((it) => `<li><b>[${it.id}] ${esc(it.l)}</b> · ${fmt(it.b).replace(' GB', '&nbsp;GB')}<code>${esc(it.p)}</code>${esc(it.why)}${it.cmd ? `<code class="cmd">${esc(it.cmd)}</code>` : ''}</li>`).join('');
  sheet.hidden = false;
  const r = li.getBoundingClientRect(), d = dig.getBoundingClientRect();
  const sw = sheet.offsetWidth, sh = sheet.offsetHeight;
  let x = narrow ? 12 : r.left - d.left - sw - 30, y = r.top - d.top - 20;
  x = Math.max(12, x); y = Math.max(64, Math.min(y, dig.clientHeight - sh - 12));
  sheet.style.left = x + 'px'; sheet.style.top = y + 'px';
  $('#sheet-x').focus();
}
function closeSheet() { if (sheet.hidden) return; sheet.hidden = true; if (opener) opener.focus(); opener = null; }
const esc = (s) => String(s ?? '').replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
$('#sheet-x').addEventListener('click', closeSheet);
document.addEventListener('keydown', (e) => { if (e.key === 'Escape') closeSheet(); });
document.addEventListener('click', (e) => { if (!sheet.hidden && !sheet.contains(e.target) && !e.target.closest('.tag')) closeSheet(); });

// ---------- deeper layers: stippled backgrounds and wavy tops ----------
function soilSections() {
  document.querySelectorAll('.layer').forEach((sec, i) => {
    const soil = [...sec.classList].find((c) => c.startsWith('s-')).slice(2);
    const t = texture(soil, 360, 360, 300 + i * 13);
    sec.style.backgroundImage = `url(${t.toDataURL('image/png')})`;
    const n = noise1(700 + i * 17), w = 1600; let d = `M0 18 L0 ${9 + n(0) * 6}`;
    for (let x = 0; x <= w; x += 20) d += ` L${x} ${(9 + n(x) * 6).toFixed(1)}`;
    d += ` L${w} 18 Z`;
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('class', 'edge'); svg.setAttribute('viewBox', `0 0 ${w} 18`); svg.setAttribute('preserveAspectRatio', 'none'); svg.setAttribute('aria-hidden', 'true');
    svg.innerHTML = `<path d="${d}" fill="${SOIL[soil].base}"/><path d="${d.replace(/ L1600 18 Z$/, '').replace(/^M0 18 L/, 'M')}" fill="none" stroke="rgba(21,20,19,.5)" stroke-width="1.2" vector-effect="non-scaling-stroke"/>`;
    sec.prepend(svg);
  });
}

// ---------- copy buttons ----------
document.querySelectorAll('.copy').forEach((b) => b.addEventListener('click', async () => {
  const text = document.getElementById(b.dataset.copy).textContent;
  try { await navigator.clipboard.writeText(text); b.textContent = 'copied'; b.classList.add('done'); } catch { // no clipboard (http, old browser): select the command so Ctrl/Cmd-C works
    const sel = getSelection(), rg = document.createRange(); rg.selectNodeContents(document.getElementById(b.dataset.copy)); sel.removeAllRanges(); sel.addRange(rg); b.textContent = 'press ⌘/Ctrl-C'; }
  setTimeout(() => { b.textContent = 'copy'; b.classList.remove('done'); }, 2200);
}));

// ---------- boot ----------
btn.addEventListener('click', () => { autoplayed = true; shovel(); });
fetch('demo.json').then((r) => r.json()).then(async (d) => {
  for (const it of d.macos.items) items[it.id] = it;
  freeBase = d.macos.free || freeBase;
  try { await document.fonts.ready; } catch {}
  // reduced motion: no scrape, no flying grains, no autoplay. Open on the finished dig
  // (SAFE strata gone, heap on the bank, free space back above the floor); the button backfills.
  if (reduce) state = 'done';
  build(); setFree(freeNow); intro(); if (reduce) finish(); soilSections();
  let rt = 0; let lastW = W;
  addEventListener('resize', () => { clearTimeout(rt); rt = setTimeout(() => { if (state === 'digging') return; if (Math.abs(dig.clientWidth - lastW) < 2 && !narrow) { build(); return; } lastW = dig.clientWidth; build(); }, 160); });
}).catch(() => { dig.classList.add('nojs'); });
})();
