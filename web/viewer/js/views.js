// Views: the project's plans, elevations, sections, drafting views, legends and sheets, as the desktop's Project Browser
// lists them, each a drawing the file carries (gzipped SVG, its lines grouped by the element they draw). A 2D view lies
// over the 3D one: drag pans, the wheel zooms about the cursor, two fingers pinch, a click picks the element under it (the
// selection is the model's, so the 3D view and Properties follow), and a double click on a sheet's view opens that view.
// Measure works in plans, sections and elevations, snapping to the drawing's corners; a sheet mixes scales, so not there.
// A file that carries no drawings lists the 3D view alone.
import { fold } from './panels.js';

const SVG = 'http://www.w3.org/2000/svg';
// What a drawing may hold: BIMCraft writes these and nothing else, and a file is hostile, so anything else is dropped.
const KEEP = new Set(['svg', 'g', 'path', 'rect', 'image']);
const ATTRS = new Set(['xmlns', 'width', 'height', 'viewBox', 'x', 'y', 'd', 'fill', 'stroke', 'stroke-width', 'stroke-linecap',
  'stroke-linejoin', 'stroke-dasharray', 'vector-effect', 'href', 'data-id']);
const SNAP = 10; // px

// Run on the parser's inert document, before the drawing is brought into the page, so nothing in it loads or runs first.
function clean(node) {
  for (const el of [...node.querySelectorAll('*'), node]) {
    if (el.namespaceURI !== SVG || !KEEP.has(el.localName)) { el.remove(); continue; }
    for (const a of [...el.attributes]) {
      const ok = ATTRS.has(a.name) && !(a.name === 'href' && !a.value.startsWith('data:image/png;base64,'));
      if (!ok) el.removeAttribute(a.name);
    }
  }
  return node;
}

async function gunzip(buffer) {
  const stream = new Blob([buffer]).stream().pipeThrough(new DecompressionStream('gzip'));
  return new Response(stream).text();
}

export class Views {
  constructor(app) {
    this.app = app; this.list = []; this.cache = new Map(); this.open = null; this.points = null;
    this.paper = document.getElementById('paper'); this.title = document.getElementById('papertitle');
    this.el = document.getElementById('viewlist');
    this.pointers = new Map();
    app.on('select', () => this.mark());
    app.on('visibility', () => this.shown());
    const p = this.paper;
    p.addEventListener('pointerdown', e => this.down(e));
    p.addEventListener('pointermove', e => this.move(e));
    p.addEventListener('pointerup', e => this.up(e));
    p.addEventListener('pointercancel', e => { clearTimeout(this.pointers.get(e.pointerId)?.hold); this.pointers.delete(e.pointerId); });
    p.addEventListener('wheel', e => { e.preventDefault(); this.zoom(e.clientX, e.clientY, Math.exp(e.deltaY * 0.0015)); }, { passive: false });
    p.addEventListener('dblclick', e => this.dbl(e));
    p.addEventListener('contextmenu', e => e.preventDefault());
    new ResizeObserver(() => { if (this.open) this.fit(); }).observe(p);
  }

  // The list, after a model loads: 3D first, then each group's views.
  build(list) {
    this.fill(Array.isArray(list) ? list : [], false);
  }
  fill(list, keep) {
    const was = keep && this.open ? { id: this.open.v.id, group: this.open.v.group } : null;
    this.list = list; this.cache.clear();
    if (!was) this.close();
    const rows = [this.row('3D View', -1)];
    rows[0].classList.add('first');
    const groups = new Map();
    this.list.forEach((v, k) => { if (!groups.has(v.group)) groups.set(v.group, []); groups.get(v.group).push(this.row(v.name, k)); });
    for (const [group, kids] of groups) rows.push(...fold(group, kids, false));
    if (!this.list.length) { const h = document.createElement('div'); h.className = 'hint'; h.textContent = 'This file carries no plans, sections or sheets.'; rows.push(h); }
    this.el.replaceChildren(...rows); this.rows();
    document.getElementById('b-views').disabled = false;
    if (!was) return;
    const k = this.list.findIndex(v => v.id === was.id && v.group === was.group);
    if (k < 0) this.close(); else this.show(k, true);
  }
  row(name, k) {
    const b = document.createElement('button'); b.type = 'button'; b.className = 'vrow'; b.textContent = name; b.dataset.k = k;
    b.onclick = () => { if (k < 0) this.close(); else this.show(k); if (window.innerWidth <= 700) this.app.panel('views', false); };
    return b;
  }
  // The open view, lit in the list and named on the panel's name line.
  rows() {
    this.el.querySelectorAll('.vrow').forEach(b => b.classList.toggle('on', +b.dataset.k === (this.open ? this.open.k : -1)));
    const v = this.open?.v, name = document.getElementById('viewname');
    name.textContent = !v ? '3D View' : v.group === 'Sheets' ? v.name : `${v.name} · ${v.group.replace(/s$/, '')}`; name.title = name.textContent;
  }

  // A view's drawing, from the file: unzipped, cleaned, kept for the next time.
  async drawing(k) {
    if (this.cache.has(k)) return this.cache.get(k);
    const v = this.list[k];
    let text;
    if (v.bufferView != null) text = await gunzip(await this.app.parser.getDependency('bufferView', v.bufferView));
    else throw new Error('this file carries no drawing of it');
    const doc = new DOMParser().parseFromString(text, 'image/svg+xml');
    const svg = doc.documentElement;
    if (svg.namespaceURI !== SVG || svg.localName !== 'svg') throw new Error('not a drawing');
    const node = document.importNode(clean(svg), true);
    node.setAttribute('preserveAspectRatio', 'none'); node.removeAttribute('width'); node.removeAttribute('height');
    const marks = document.createElementNS(SVG, 'g'); marks.setAttribute('class', 'marks'); node.append(marks);
    this.cache.set(k, node);
    return node;
  }

  // `again`: the open view drawn again after a change, kept where it is (panned, zoomed, measured); else it is fitted.
  async show(k, again = false) {
    const app = this.app, v = this.list[k]; if (!v) return;
    if (app.tool && !again) app.setTool(null);
    // Only the last view asked for opens: a large drawing takes a while to read, and another may be clicked meanwhile.
    const ticket = this.ticket = (this.ticket || 0) + 1;
    let node;
    try { node = await this.drawing(k); } catch (e) {
      if (ticket === this.ticket) app.tip(again ? `${v.name} could not be drawn again: ${e.message}` : `${v.name} could not be read: ${e.message}`);
      return;
    }
    if (ticket !== this.ticket) return;
    const vb = again && this.open ? [...this.open.vb] : null;
    this.open = { k, v, node, vb: vb || [0, 0, v.width, v.height] }; this.points = null;
    this.paper.replaceChildren(node); this.paper.hidden = false; this.title.hidden = false;
    this.title.textContent = `${v.group.replace(/s$/, '')} · ${v.name}`;
    document.body.classList.add('in-2d');
    for (const id of ['b-walk', 'b-section', 'b-look']) document.getElementById(id).disabled = true;
    document.getElementById('b-measure').disabled = v.group === 'Sheets';
    for (const id of ['secpop', 'lookpop']) document.getElementById(id).hidden = true;
    if (vb) { this.set(); this.marksMoved(); } else this.fit();
    this.mark(); this.shown(); this.rows();
    app.emit('view2d', v);
  }
  close() {
    this.ticket = (this.ticket || 0) + 1;
    if (!this.open) { this.rows(); return; }
    if (this.app.tool) this.app.setTool(null);
    this.open = null; this.paper.hidden = true; this.title.hidden = true; this.paper.replaceChildren();
    document.body.classList.remove('in-2d');
    for (const id of ['b-walk', 'b-section', 'b-look', 'b-measure']) document.getElementById(id).disabled = false;
    this.rows(); this.app.emit('view2d', null); this.app.redraw();
  }

  // The view box: the drawing's units across the paper, kept to the paper's shape so nothing stretches.
  set() { const [x, y, w, h] = this.open.vb; this.open.node.setAttribute('viewBox', `${x} ${y} ${w} ${h}`); }
  fit() {
    const o = this.open; if (!o) return;
    const r = this.paper.getBoundingClientRect(); if (!r.width) return;
    // In the part the panels and the tool bar leave free, under the title.
    const f = this.app.free(), y0 = Math.min(f.y0 + 46, f.y1 / 2), pad = 0.03, fw = f.x1 - f.x0, fh = f.y1 - y0;
    const s = Math.max(o.v.width / (fw * (1 - 2 * pad)), o.v.height / (fh * (1 - 2 * pad)));
    const cx = (f.x0 + f.x1) / 2, cy = (y0 + f.y1) / 2;
    o.vb = [o.v.width / 2 - cx * s, o.v.height / 2 - cy * s, r.width * s, r.height * s]; this.set(); this.marksMoved();
  }
  // A screen point in the drawing's units, and back.
  at(x, y) { const o = this.open, r = this.paper.getBoundingClientRect(); return [o.vb[0] + (x - r.left) / r.width * o.vb[2], o.vb[1] + (y - r.top) / r.height * o.vb[3]]; }
  screen([u, v]) { const o = this.open, r = this.paper.getBoundingClientRect(); return [r.left + (u - o.vb[0]) / o.vb[2] * r.width, r.top + (v - o.vb[1]) / o.vb[3] * r.height]; }
  zoom(x, y, f) {
    const o = this.open; if (!o) return;
    const [u, v] = this.at(x, y), w = Math.min(Math.max(o.vb[2] * f, o.v.width / 400), o.v.width * 20), k = w / o.vb[2];
    o.vb = [u - (u - o.vb[0]) * k, v - (v - o.vb[1]) * k, w, o.vb[3] * k]; this.set(); this.marksMoved();
  }

  down(e) {
    this.paper.setPointerCapture(e.pointerId);
    const p = { x: e.clientX, y: e.clientY, sx: e.clientX, sy: e.clientY, moved: false };
    this.pointers.set(e.pointerId, p);
    // A finger has no right button: held still, it opens the same menu, as in the 3D view.
    if (e.pointerType === 'touch') p.hold = setTimeout(() => { if (!p.moved && this.pointers.size === 1 && this.open && this.app.tool !== 'measure') { p.held = true; this.menu(e); } }, 500);
  }
  menu(e) { this.app.openPlanMenu(this.app.byId.get(this.under(e.clientX, e.clientY)) || null, e.clientX, e.clientY); }
  move(e) {
    const p = this.pointers.get(e.pointerId);
    if (!p) { this.hover(e); return; }
    if (Math.hypot(e.clientX - p.sx, e.clientY - p.sy) > 4) p.moved = true;
    const o = this.open; if (!o) return;
    if (this.pointers.size === 2) {
      const [a, b] = [...this.pointers.values()], before = Math.hypot(a.x - b.x, a.y - b.y);
      p.x = e.clientX; p.y = e.clientY;
      const after = Math.hypot(a.x - b.x, a.y - b.y);
      if (before > 0 && after > 0) this.zoom((a.x + b.x) / 2, (a.y + b.y) / 2, before / after);
      return;
    }
    const r = this.paper.getBoundingClientRect();
    o.vb[0] -= (e.clientX - p.x) / r.width * o.vb[2]; o.vb[1] -= (e.clientY - p.y) / r.height * o.vb[3];
    p.x = e.clientX; p.y = e.clientY; this.set(); this.marksMoved();
  }
  up(e) {
    const p = this.pointers.get(e.pointerId); this.pointers.delete(e.pointerId);
    if (p) clearTimeout(p.hold);
    if (!p || p.moved || p.held || !this.open) return;
    if (e.button === 2 && this.app.tool !== 'measure') return this.menu(e);
    if (e.button !== 0) return;
    if (this.app.tool === 'measure') return this.measureClick(e);
    const id = this.under(e.clientX, e.clientY);
    this.pick(id);
  }
  dbl(e) {
    const o = this.open; if (!o) return;
    const id = this.under(e.clientX, e.clientY);
    const port = (o.v.viewports || []).find(p => p.id === id);
    if (port && port.view != null) return this.show(port.view);
    if (id == null) this.fit();
  }

  // The element whose lines are under a screen point, or near it: a hairline is one pixel wide, so a ring round the point
  // is tried too.
  under(x, y) {
    const at = (px, py) => {
      const el = document.elementFromPoint(px, py), g = el?.closest?.('#paper g[data-id]');
      return g ? +g.dataset.id : null;
    };
    let id = at(x, y);
    for (let r = 3; id == null && r <= 6; r += 3) for (let a = 0; a < 8 && id == null; a++) id = at(x + r * Math.cos(a * Math.PI / 4), y + r * Math.sin(a * Math.PI / 4));
    return id;
  }
  pick(id) {
    const app = this.app, node = id == null ? null : app.byId.get(id);
    this.only = node || id == null ? null : id;
    app.select(node || null);
    if (this.only != null) { this.mark(); app.tip(this.portOf(id) ? 'A view on the sheet: double-click to open it.' : `Element ${id}: drawn in this view only, not in the 3D model.`); }
    else app.tip(null);
  }
  portOf(id) { return (this.open?.v.viewports || []).find(p => p.id === id); }
  hover(e) {
    if (!this.open) return;
    if (this.app.tool === 'measure') return this.measureHover(e);
    const id = this.under(e.clientX, e.clientY);
    if (id === this.hovered) return;
    this.hovered = id;
    this.open.node.querySelectorAll('g.hover').forEach(g => g.classList.remove('hover'));
    if (id != null) this.open.node.querySelectorAll(`g[data-id="${id}"]`).forEach(g => g.classList.add('hover'));
    this.paper.style.cursor = id != null ? 'pointer' : '';
  }
  // The selection, and what is hidden, as the model has them.
  mark() {
    const o = this.open; if (!o) return;
    const id = this.app.selected ? this.app.selected.userData.id : this.only;
    if (this.app.selected) this.only = null;
    o.node.querySelectorAll('g.sel').forEach(g => g.classList.remove('sel'));
    if (id != null) o.node.querySelectorAll(`g[data-id="${id}"]`).forEach(g => g.classList.add('sel'));
  }
  shown() {
    const o = this.open; if (!o) return;
    o.node.querySelectorAll('g[data-id]').forEach(g => { const n = this.app.byId.get(+g.dataset.id); g.classList.toggle('off', !!n && !n.visible); });
  }

  // Measure: two clicks, each snapped to the nearest corner of the drawing within a few pixels, the length in mm by the
  // view's scale.
  measureStart() { this.pts = []; this.clearMarks(); this.app.tip('Measure: click two points; corners snap. Esc stops.'); }
  measureStop() { this.pts = []; this.clearMarks(); this.app.tip(null); }
  corners() {
    if (this.points) return this.points;
    const out = [];
    for (const p of this.open.node.querySelectorAll('path')) {
      const nums = (p.getAttribute('d') || '').match(/-?\d+(\.\d+)?/g) || [];
      for (let i = 0; i + 1 < nums.length; i += 2) out.push(+nums[i], +nums[i + 1]);
    }
    return (this.points = Float32Array.from(out));
  }
  snap(x, y) {
    const [u, v] = this.at(x, y), pts = this.corners(), r = SNAP / this.paper.getBoundingClientRect().width * this.open.vb[2];
    let best = null, d2 = r * r;
    for (let i = 0; i < pts.length; i += 2) { const dx = pts[i] - u, dy = pts[i + 1] - v, d = dx * dx + dy * dy; if (d < d2) { d2 = d; best = [pts[i], pts[i + 1]]; } }
    return { p: best || [u, v], snapped: !!best };
  }
  measureHover(e) { const s = this.snap(e.clientX, e.clientY); this.drawMarks(s.p, s.snapped); }
  measureClick(e) {
    const s = this.snap(e.clientX, e.clientY);
    if (this.pts.length >= 2) this.pts = [];
    this.pts.push(s.p); this.drawMarks(null, false);
  }
  clearMarks() { this.open?.node.querySelector('.marks')?.replaceChildren(); document.querySelectorAll('.label.m2d').forEach(l => l.remove()); }
  drawMarks(cursor, snapped) {
    const o = this.open; if (!o) return;
    this.clearMarks();
    const g = o.node.querySelector('.marks'), u = o.vb[2] / this.paper.getBoundingClientRect().width;
    const add = (tag, attrs) => { const el = document.createElementNS(SVG, tag); for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v); g.append(el); };
    const dot = (p, ring) => add('circle', { cx: p[0], cy: p[1], r: (ring ? 6 : 4) * u, fill: ring ? 'none' : '#1c5ca3', stroke: '#1c5ca3', 'stroke-width': 1.5 * u });
    const pts = [...this.pts]; if (cursor && pts.length === 1) pts.push(cursor);
    if (pts.length === 2) {
      add('path', { d: `M${pts[0][0]} ${pts[0][1]} L${pts[1][0]} ${pts[1][1]}`, stroke: '#1c5ca3', 'stroke-width': 2 * u, fill: 'none' });
      const mm = Math.hypot(pts[1][0] - pts[0][0], pts[1][1] - pts[0][1]) / o.v.scale;
      const [x, y] = this.screen([(pts[0][0] + pts[1][0]) / 2, (pts[0][1] + pts[1][1]) / 2]), r = this.app.view.getBoundingClientRect();
      const l = document.createElement('div'); l.className = 'label m2d'; l.textContent = `${Math.round(mm).toLocaleString()} mm`;
      l.style.left = (x - r.left) + 'px'; l.style.top = (y - r.top) + 'px'; this.app.view.append(l);
    }
    this.pts.forEach(p => dot(p, false));
    if (cursor) dot(cursor, snapped);
  }
  marksMoved() { if (this.app.tool === 'measure' && this.pts?.length) this.drawMarks(null, false); }
}
