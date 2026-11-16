// Measure: distances and angles picked on the model, kept until deleted or cleared. A point snaps, within 12 px, to the nearest
// corner, edge midpoint, circle centre or crossing of two edges, else to an edge within 8 px, else lands on the face under the
// cursor (`snaps.js`); what it will snap to shows as that kind's shape, and the face is lit, before the click. Each end is drawn
// in the style picked in Settings, a dot or a small cross (`ends.js`). A
// distance shows its length on the line; the one picked (the newest, or one whose label is clicked) also shows its parts
// east, north and up as red, green and blue legs, each named beside its own leg, and none for a leg too short to read.
// Lengths are in the unit and precision of Settings, kept in the browser.
//
// The lines, points and labels are drawn over the picture each frame (an SVG and the labels' own elements), so they are the same
// width and size at any distance, and the labels are laid again each frame (`labels.js`) so none sits on another as the view
// moves. A placed point is dragged to move it (it snaps again); a distance's label is dragged to pull its line off the model,
// with lines out to the points, as a drawn dimension. A double-click on a flat face shows its sizes: the length of each edge,
// its area and, where it has one, its thickness (`faces.js`); Esc or a click clears them.
//
// What is under the pointer is found once a frame, at its last place; a press that moves the view hides the snap's shape and the lit face,
// and they are found again where the pointer is when it lets go or the view moves under it (`hover.js`). The lit face is left out
// of the edges' ink (`userData.inkless`, `look.js`).
import * as THREE from 'three';
import { layout, along, beside } from './labels.js';
import { outline, onFace } from './faces.js';
import { Hover } from './hover.js';
import { snapAt, snapMark } from './snaps.js';
import { endMark, endStyle, ways } from './ends.js';
import { tf } from './words.js';

const UNITS = {
  mm: { name: 'Millimetres', k: 1000, sym: 'mm', places: [0, 1, 2] },
  cm: { name: 'Centimetres', k: 100, sym: 'cm', places: [0, 1, 2] },
  m: { name: 'Metres', k: 1, sym: 'm', places: [2, 3, 4] },
  ftin: { name: 'Feet and inches', fractions: [1, 2, 4, 8, 16] },
  in: { name: 'Inches', fractions: [1, 2, 4, 8, 16] },
  ft: { name: 'Decimal feet', k: 1 / 0.3048, sym: 'ft', places: [1, 2, 3] },
};
// The legs east, north and up: the Autodesk viewer's red and blue, its green a shade darker so each keeps 3:1 on its white
// halo over the model; solid, LEG px wide (with the halo under it, as the main line).
export const AXES = [['x', '#f12c2c', 'East'], ['y', '#0a9f0a', 'North'], ['z', '#2c2cf1', 'Up']];
export const LEG = 2;
// A leg's name: a white pill in its own colour's words (`.mtag` in index.html).
export const tagClass = axis => 'label mtag ' + axis;
// The snap indicator, under the pointer and at each placed point: the Autodesk viewer's snap orange, edged by a thin dark
// keyline, so it reads on a light face by its keyline and on a dark one by its fill. A line's white halo adds HALO px.
const SNAP = '#ff7700';
const KEYLINE = '#1a1a1a';
const KEYLINE_W = 1;
export const HALO = 1.5;
export const HALO_INK = 'rgba(255,255,255,.85)';
// The face under the pointer, lit as the Autodesk viewer lights a snapped face: green at half strength.
const LIT = '#00cc00';
const LIT_OPACITY = 0.5;
// Every snap shape and end mark (`snaps.js`, `ends.js`): the snap colour, edged by the keyline on each side.
const MARK = { ink: SNAP, halo: KEYLINE, haloW: 2 * KEYLINE_W, ring: KEYLINE_W };
const grouped = s => s.replace(/\B(?=(\d{3})+(?!\d))/g, ' ');

// A length in metres as the settings show it.
export function length(m, unit, prec) {
  const u = UNITS[unit] || UNITS.mm, sign = m < 0 ? '−' : '';
  if (u.fractions) {
    let n = Math.round(Math.abs(m) / 0.0254 * prec), ft = 0;
    if (unit === 'ftin') { ft = Math.floor(n / (12 * prec)); n -= ft * 12 * prec; }
    const whole = Math.floor(n / prec); let a = n % prec, b = prec;
    while (a && a % 2 === 0) { a /= 2; b /= 2; }
    const inch = (whole || !a ? String(whole) : '') + (a ? (whole ? ' ' : '') + a + '/' + b : '') + '"';
    return sign + (unit === 'ftin' ? `${ft}' ${inch}` : inch);
  }
  const [i, f] = (Math.abs(m) * u.k).toFixed(prec).split('.');
  return sign + grouped(i) + (f ? '.' + f : '') + ' ' + u.sym;
}

export class Measure {
  constructor(app) {
    this.app = app; this.mode = 'distance'; this.pts = []; this.items = []; this.picked = null; this.hist = []; this.sized = null;
    try { const s = JSON.parse(localStorage.getItem('bimcraft-viewer-units') || '{}'); this.unit = UNITS[s.unit] ? s.unit : 'mm'; this.prec = s.prec; } catch { this.unit = 'mm'; }
    if (!this.precs().includes(this.prec)) this.prec = this.precs()[0];
    // The snap under the pointer ({ p, kind, angle }), drawn as its kind's shape; the end style picked in Settings (`ends.js`).
    this.cur = null; this.ends = endStyle(); this.faces = new Map(); this.v = new THREE.Vector3();
    // The lit face goes in the model's scene, so what stands in front of it still hides it.
    const lit = () => new THREE.MeshBasicMaterial({ color: LIT, transparent: true, opacity: LIT_OPACITY,
      depthWrite: false, polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -2, side: THREE.DoubleSide });
    this.face = new THREE.Mesh(new THREE.BufferGeometry(), lit());
    this.face.visible = false; this.face.renderOrder = 5; app.scene.add(this.face);
    this.sizedFace = new THREE.Mesh(new THREE.BufferGeometry(), lit());
    this.sizedFace.visible = false; this.sizedFace.renderOrder = 5; app.scene.add(this.sizedFace);
    this.face.userData.inkless = this.sizedFace.userData.inkless = true;
    this.hov = new Hover(); this.downs = new Set();
    const c = app.canvas;
    // Before the page's own handlers (this is made first): a press that moves the view takes the dot and the lit face away.
    c.addEventListener('pointerdown', e => {
      if (app.tool !== 'measure') return;
      this.downs.add(e.pointerId); this.hov.press();
      if (!this.near(e)) this.unhover();
    });
    const up = e => {
      if (!this.downs.delete(e.pointerId) || this.downs.size) return;
      this.hov.release(e.clientX, e.clientY);
      if (e.pointerType === 'touch') this.hov.leave();
    };
    c.addEventListener('pointerup', up); c.addEventListener('pointercancel', up);
    c.addEventListener('pointerleave', () => { if (this.downs.size) return; this.hov.leave(); if (app.tool === 'measure') this.unhover(); });
    app.on('tick', () => { if (app.tool !== 'measure') return; const at = this.hov.take(); if (at) this.under({ clientX: at.x, clientY: at.y }); });
    this.svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); this.svg.id = 'measvg'; app.view.append(this.svg);
    this.bar = document.getElementById('measbar'); this.set = document.getElementById('measset');
    this.bar.addEventListener('click', e => {
      const b = e.target.closest('button'); if (!b) return;
      const a = b.dataset.m;
      if (a === 'distance' || a === 'angle') { this.mode = a; this.cancel(); this.hint(); this.buttons(); }
      else if (a === 'delete') this.remove(this.picked);
      else if (a === 'clear') this.clear();
      else if (a === 'settings') { this.set.hidden = !this.set.hidden; this.buttons(); }
    });
    this.settings();
    app.on('drawn', () => this.place());
    app.on('camera', () => this.hov.view());
  }
  precs() { const u = UNITS[this.unit]; return u.fractions || u.places; }
  fmt(m) { return length(m, this.unit, this.prec); }
  area(m2) { return UNITS[this.unit].fractions || this.unit === 'ft' ? (m2 / 0.09290304).toFixed(2) + ' ft²' : m2.toFixed(2) + ' m²'; }
  // Settings: the unit and how fine; every label is written again when they change.
  settings() {
    const unit = document.getElementById('m-unit'), prec = document.getElementById('m-prec');
    unit.replaceChildren(...Object.entries(UNITS).map(([k, u]) => new Option(u.name, k, false, k === this.unit)));
    const fill = () => prec.replaceChildren(...this.precs().map(p => new Option(UNITS[this.unit].fractions ? (p === 1 ? '1"' : `1/${p}"`) : (p ? '0.' + '0'.repeat(p) : '0'), p, false, p === this.prec)));
    fill();
    const save = () => { try { localStorage.setItem('bimcraft-viewer-units', JSON.stringify({ unit: this.unit, prec: this.prec })); } catch { /* the browser keeps nothing */ } this.relabel(); };
    unit.onchange = () => { this.unit = unit.value; this.prec = this.precs()[0]; fill(); save(); };
    prec.onchange = () => { this.prec = Number(prec.value); save(); };
  }
  start() { this.bar.hidden = false; this.set.hidden = true; this.buttons(); this.hint(); }
  stop() {
    this.clear(); this.bar.hidden = this.set.hidden = true; this.cur = null; this.face.visible = false;
    this.app.tip(null); this.app.redraw();
  }
  hint() {
    const n = this.pts.length;
    this.app.tip(this.mode === 'angle' ? ['Angle: click the first point.', 'Angle: click the corner.', 'Angle: click the last point.'][n]
      : ['Distance: click the first point.', 'Distance: click the second point.'][n]);
  }
  buttons() {
    this.bar.querySelectorAll('button').forEach(b => {
      const a = b.dataset.m;
      if (a === 'distance' || a === 'angle') b.setAttribute('aria-pressed', String(a === this.mode));
      if (a === 'settings') b.setAttribute('aria-pressed', String(!this.set.hidden));
      if (a === 'delete') b.disabled = !this.picked;
      if (a === 'clear') b.disabled = !this.items.length && !this.pts.length;
    });
  }
  screen(p) {
    const r = this.app.size(), v = p.clone().project(this.app.camera);
    return { x: (v.x + 1) / 2 * r.w, y: (1 - v.y) / 2 * r.h, behind: v.z > 1 };
  }
  // A line from `a` to `b` on the screen, cut where it passes behind the eye; null where none of it is in front.
  seen(a, b) {
    const cam = this.app.camera, r = this.app.size(), near = -(cam.near + 1e-4);
    const va = a.clone().applyMatrix4(cam.matrixWorldInverse), vb = b.clone().applyMatrix4(cam.matrixWorldInverse);
    if (va.z > near && vb.z > near) return null;
    if (va.z > near) va.lerp(vb, (va.z - near) / (va.z - vb.z)); else if (vb.z > near) vb.lerp(va, (vb.z - near) / (vb.z - va.z));
    const at = v => { v.applyMatrix4(cam.projectionMatrix); return { x: (v.x + 1) / 2 * r.w, y: (1 - v.y) / 2 * r.h }; };
    return [at(va), at(vb)];
  }
  // The point a cursor means (`snaps.js`): within 12 px the nearest corner, edge midpoint, circle centre or place where two edges
  // cross on the screen, else within 8 px the nearest point of an edge, else the face. The edges are the outline of the flat face
  // hit; near one of them, those of the faces beside it too, which a crossing needs.
  snap(e) {
    const h = this.app.pick(e.clientX, e.clientY); if (!h || !h.face) return null;
    const r = this.app.size(), at = { x: e.clientX - r.left, y: e.clientY - r.top }, to = p => this.toScreen(p);
    const edges = h.normal ? this.faceOf(h).edges : this.sides(h), more = this.nearEdge(edges, at) ? this.beside(e, h) : [];
    const s = snapAt(at, { point: h.point.toArray(), edges, more }, to);
    return s && { p: new THREE.Vector3(s.p[0], s.p[1], s.p[2]), kind: s.kind, angle: s.angle, hit: h };
  }
  // The sides of the triangle hit, for a hit that has no face's way.
  sides(h) {
    const pos = h.object.geometry.attributes.position, M = h.object.matrixWorld;
    const vs = [h.face.a, h.face.b, h.face.c].map(i => new THREE.Vector3().fromBufferAttribute(pos, i).applyMatrix4(M).toArray());
    return [[vs[0], vs[1]], [vs[1], vs[2]], [vs[2], vs[0]]];
  }
  toScreen(p) {
    const r = this.app.size(), v = this.v.set(p[0], p[1], p[2]).project(this.app.camera);
    return { x: (v.x + 1) / 2 * r.w, y: (1 - v.y) / 2 * r.h, z: v.z, behind: v.z > 1 };
  }
  // Whether the pointer is within 12 px of one of the edges.
  nearEdge(edges, at) {
    return edges.some(([a, b]) => {
      const p = this.toScreen(a), q = this.toScreen(b), dx = q.x - p.x, dy = q.y - p.y, l2 = dx * dx + dy * dy;
      const t = l2 > 0 ? Math.max(0, Math.min(1, ((at.x - p.x) * dx + (at.y - p.y) * dy) / l2)) : 0;
      return Math.hypot(p.x + t * dx - at.x, p.y + t * dy - at.y) <= 12;
    });
  }
  // The edges of the flat faces 10 px round the pointer that are not the one hit.
  beside(e, h) {
    const keys = new Set([Measure.key(h)]), out = [];
    for (const [dx, dy] of [[10, 0], [-10, 0], [0, 10], [0, -10]]) {
      const b = this.app.pick(e.clientX + dx, e.clientY + dy); if (!b || !b.face || !b.normal) continue;
      const k = Measure.key(b); if (keys.has(k)) continue;
      keys.add(k); out.push(...this.faceOf(b).edges);
    }
    return out;
  }
  // The flat face hit: its triangles and its outline's edges ([a, b], points [x, y, z]), kept for the last 32 faces met.
  faceOf(h) {
    const key = Measure.key(h); let f = this.faces.get(key);
    if (f) return f;
    const tris = this.plane(h);
    f = { tris, edges: outline(tris, 0).segments };
    if (this.faces.size >= 32) this.faces.delete(this.faces.keys().next().value);
    this.faces.set(key, f);
    return f;
  }
  // A placed point within 8 px of the cursor: { it, i }, or null.
  near(e) {
    const r = this.app.size(), cx = e.clientX - r.left, cy = e.clientY - r.top;
    let best = null, bd = 8;
    for (const it of this.items) it.pts.forEach((p, i) => {
      const s = this.screen(p), d = Math.hypot(s.x - cx, s.y - cy);
      if (!s.behind && d < bd) { bd = d; best = { it, i }; }
    });
    return best;
  }
  // The pointer moved with nothing pressed: what is under it is found on the next frame.
  hover(e) { this.hov.move(e.clientX, e.clientY); }
  unhover() { this.cur = null; this.light(null); this.live = null; this.app.redraw(); }
  under(e) {
    if (this.held) return;
    if (this.near(e)) { this.cur = null; this.light(null); this.app.canvas.style.cursor = 'grab'; this.live = null; this.app.redraw(); return; }
    const s = this.snap(e);
    this.cur = s;
    if (s) this.app.canvas.style.cursor = s.kind === 'face' ? 'crosshair' : 'cell';
    this.light(s?.hit);
    this.live = this.pts.length && s ? s.p.clone() : null;
    this.app.redraw();
  }
  // The triangles of the flat face hit, in the world, nine numbers each: those of that shape on the same plane as the one hit and
  // facing its way, the one hit first. Past 600 000 corners only the one hit, as a shape that size is too slow to search under a
  // moving cursor.
  plane(h) {
    const n = h.normal, d = n.dot(h.point);
    const g = h.object.geometry, idx = g.index, pos = g.attributes.position, M = h.object.matrixWorld;
    const count = idx ? idx.count : pos.count, a = new THREE.Vector3(), b = new THREE.Vector3(), c = new THREE.Vector3();
    const A = [], B = [], C = [], N = n.toArray();
    const out = [...[h.face.a, h.face.b, h.face.c].flatMap(i => a.fromBufferAttribute(pos, i).applyMatrix4(M).toArray())];
    if (count > 600000) return out;
    const seed = h.faceIndex * 3;
    for (let i = 0; i < count; i += 3) {
      if (i === seed) continue;
      const i0 = idx ? idx.getX(i) : i, i1 = idx ? idx.getX(i + 1) : i + 1, i2 = idx ? idx.getX(i + 2) : i + 2;
      a.fromBufferAttribute(pos, i0).applyMatrix4(M);
      if (Math.abs(n.dot(a) - d) > 0.002) continue;
      b.fromBufferAttribute(pos, i1).applyMatrix4(M); c.fromBufferAttribute(pos, i2).applyMatrix4(M);
      if (!onFace(a.toArray(A), b.toArray(B), c.toArray(C), N, d)) continue;
      out.push(a.x, a.y, a.z, b.x, b.y, b.z, c.x, c.y, c.z);
    }
    return out;
  }
  static key(h) { return h.object.id + ':' + h.normal.toArray().map(v => v.toFixed(3)) + ':' + h.normal.dot(h.point).toFixed(3); }
  // The flat face under the cursor, lit.
  light(h) {
    if (!h) { this.face.visible = false; this.faceKey = null; return; }
    const key = Measure.key(h);
    this.face.visible = true;
    if (key === this.faceKey) return;
    this.faceKey = key;
    this.face.geometry.dispose();
    this.face.geometry = new THREE.BufferGeometry(); this.face.geometry.setAttribute('position', new THREE.Float32BufferAttribute(this.faceOf(h).tris, 3));
  }
  cancel() { this.pts = []; this.live = null; this.buttons(); this.app.redraw(); }
  click(e) {
    // A press that let go of a placed point without moving it picks that point's measurement.
    if (this.grabbed) { const it = this.grabbed; this.grabbed = null; this.pick(it); return; }
    this.unsize();
    const s = this.snap(e); if (!s) return;
    // What the click undoes if it was half of a double-click on a face.
    this.hist = [...this.hist.slice(-1), { at: performance.now(), pts: this.pts.map(p => p.clone()), n: this.items.length }];
    this.pts.push(s.p.clone());
    if (this.pts.length < (this.mode === 'angle' ? 3 : 2)) { this.hint(); this.buttons(); this.app.redraw(); return; }
    const pts = this.pts; this.pts = []; this.live = null;
    this.add(this.mode, pts);
    this.hint();
  }
  // One kept measurement: its points, how far its line is pulled off them (`off`, a distance's), its label and, picked, its parts.
  add(kind, pts) {
    const it = { kind, pts, off: new THREE.Vector3(), tags: [] };
    if (kind === 'angle') this.arc(it);
    it.label = document.createElement('button'); it.label.type = 'button'; it.label.className = 'label mlabel';
    this.dragLabel(it);
    this.app.view.append(it.label);
    this.items.push(it); this.write(it); this.pick(it);
    this.app.emit('measured', kind === 'angle' ? { angle: this.angle(pts) } : { a: pts[0], b: pts[1], distance: pts[0].distanceTo(pts[1]) });
    this.app.redraw();
  }
  arc(it) {
    const [a, o, c] = it.pts, u = a.clone().sub(o), w = c.clone().sub(o), r = Math.min(u.length(), w.length()) * 0.3;
    u.normalize(); w.normalize();
    it.arc = []; for (let i = 0; i <= 24; i++) it.arc.push(o.clone().add(u.clone().lerp(w, i / 24).normalize().multiplyScalar(r)));
    it.at = it.arc[12];
  }
  // A label pressed and let go picks its measurement; dragged, it pulls a distance's line off the model.
  dragLabel(it) {
    const el = it.label;
    el.addEventListener('pointerdown', e => {
      if (e.button !== 0) return;
      e.stopPropagation(); el.setPointerCapture(e.pointerId);
      const start = { x: e.clientX, y: e.clientY, moved: false };
      const move = ev => {
        if (!start.moved && Math.hypot(ev.clientX - start.x, ev.clientY - start.y) < 4) return;
        start.moved = true; el.classList.add('held');
        if (it.kind === 'distance') { this.pullTo(it, ev); this.app.redraw(); }
      };
      const up = () => {
        el.removeEventListener('pointermove', move); el.removeEventListener('pointerup', up); el.removeEventListener('pointercancel', up);
        el.classList.remove('held');
        if (!start.moved) this.pick(this.picked === it ? null : it); else if (this.picked !== it) this.pick(it);
      };
      el.addEventListener('pointermove', move); el.addEventListener('pointerup', up); el.addEventListener('pointercancel', up);
    });
  }
  // The line's offset so its label is under the cursor: in the plane through the line that faces the eye most, square to the line.
  pullTo(it, e) {
    const [a, b] = it.pts, mid = a.clone().lerp(b, 0.5), d = b.clone().sub(a);
    if (d.lengthSq() < 1e-12) return;
    d.normalize();
    const ray = this.app.aim(new THREE.Raycaster(), this.app.ndc(e.clientX, e.clientY)).ray;
    const n = ray.direction.clone().addScaledVector(d, -ray.direction.dot(d));
    if (n.lengthSq() < 1e-8) return;
    const hit = ray.intersectPlane(new THREE.Plane().setFromNormalAndCoplanarPoint(n.normalize(), mid), new THREE.Vector3());
    if (!hit) return;
    const off = hit.sub(mid); off.addScaledVector(d, -off.dot(d));
    it.off.copy(off);
  }
  // A press on a placed point takes it: a drag moves it, snapping again, and a press let go where it was picks its measurement.
  down(e) {
    const n = this.near(e); if (!n) return false;
    this.held = { ...n, moved: false, x: e.clientX, y: e.clientY }; this.grabbed = null; this.app.canvas.style.cursor = 'grabbing';
    return true;
  }
  move(e) {
    const h = this.held; if (!h) return;
    if (!h.moved && Math.hypot(e.clientX - h.x, e.clientY - h.y) <= 4) return;
    h.moved = true;
    const s = this.snap(e);
    this.light(s?.hit);
    if (!s) return;
    h.it.pts[h.i].copy(s.p);
    if (h.it.kind === 'angle') this.arc(h.it);
    this.write(h.it); if (this.picked === h.it) this.pick(h.it);
    this.app.redraw();
  }
  up() {
    const h = this.held; this.held = null;
    if (!h) return;
    this.grabbed = h.moved ? null : h.it;
    if (h.moved) this.app.emit('measured', h.it.kind === 'angle' ? { angle: this.angle(h.it.pts) } : { a: h.it.pts[0], b: h.it.pts[1], distance: h.it.pts[0].distanceTo(h.it.pts[1]) });
    this.light(null); this.app.redraw();
  }
  angle([a, o, c]) { return THREE.MathUtils.radToDeg(a.clone().sub(o).angleTo(c.clone().sub(o))); }
  write(it) { it.label.textContent = it.kind === 'angle' ? this.angle(it.pts).toFixed(1) + '°' : this.fmt(it.pts[0].distanceTo(it.pts[1])); it.label.size = null; }
  // The picked distance's parts: east, then north, then up, from its first point to its last, each leg with its tag.
  pick(it) {
    for (const x of this.items) { x.label.classList.toggle('on', x === it); x.tags.forEach(t => t.el.remove()); x.tags = []; }
    this.picked = it;
    if (it && it.kind === 'distance') {
      const [a, b] = it.pts, p1 = new THREE.Vector3(b.x, a.y, a.z), p2 = new THREE.Vector3(b.x, a.y, b.z);
      const legs = [[a, p1, b.x - a.x], [p1, p2, a.z - b.z], [p2, b, b.y - a.y]], whole = a.distanceTo(b);
      legs.forEach(([p, q, v], i) => {
        // A leg under a twentieth of the whole says nothing worth reading.
        if (Math.abs(v) < 1e-4 || Math.abs(v) < whole * 0.05) return;
        const el = document.createElement('div'); el.className = tagClass(AXES[i][0]);
        el.textContent = AXES[i][0] + ' ' + this.fmt(Math.abs(v)); el.title = AXES[i][2];
        this.app.view.append(el); it.tags.push({ el, a: p, b: q, axis: i });
      });
    }
    this.buttons(); this.app.redraw();
  }
  remove(it) {
    if (!it) return;
    it.label.remove(); it.tags.forEach(t => t.el.remove());
    this.items = this.items.filter(x => x !== it);
    this.pick(this.items[this.items.length - 1] || null);
  }
  clear() { this.cancel(); this.unsize(); [...this.items].forEach(it => { it.label.remove(); it.tags.forEach(t => t.el.remove()); }); this.items = []; this.pick(null); }
  relabel() { this.items.forEach(it => this.write(it)); this.pick(this.picked); if (this.sized) this.resize(); }
  // Esc lets go of the points being picked, else of a face's sizes; Delete removes the picked measurement.
  key(e) {
    if (e.key === 'Escape' && this.pts.length) { this.cancel(); this.hint(); return true; }
    if (e.key === 'Escape' && this.sized) { this.unsize(); return true; }
    if ((e.key === 'Delete' || e.key === 'Backspace') && this.picked) { this.remove(this.picked); return true; }
    return false;
  }

  // A double-click on a flat face: its edges' lengths, its area and its thickness. The two clicks it is made of are undone first.
  sizes(e) {
    const now = performance.now(), was = this.hist.find(h => now - h.at < 700);
    this.hist = [];
    if (was) { while (this.items.length > was.n) this.remove(this.items[this.items.length - 1]); this.pts = was.pts; this.live = null; this.hint(); this.buttons(); }
    this.unsize();
    const h = this.app.pick(e.clientX, e.clientY);
    if (!h || !h.face || !h.normal) { this.app.redraw(); return; }
    const tris = this.plane(h), o = outline(tris, 0);
    const v = p => new THREE.Vector3(p[0], p[1], p[2]);
    this.sized = { segments: o.segments.map(([a, b]) => ({ a: v(a), b: v(b) })), area: o.area, centre: v(o.centre), thick: this.thickness(h) };
    this.sizedFace.geometry.dispose();
    this.sizedFace.geometry = new THREE.BufferGeometry(); this.sizedFace.geometry.setAttribute('position', new THREE.Float32BufferAttribute(tris, 3));
    this.sizedFace.visible = true;
    this.resize();
  }
  // The labels of the face's sizes, written again (Settings changed, or a new face).
  resize() {
    const z = this.sized; this.unlabel();
    const tag = (cls, text) => { const el = document.createElement('div'); el.className = 'label ' + cls; el.textContent = text; this.app.view.append(el); return el; };
    z.segments.forEach(s => { s.el = tag('msize', this.fmt(s.a.distanceTo(s.b))); });
    z.areaEl = tag('marea', this.area(z.area));
    z.thickEl = z.thick ? tag('marea mthick', tf('{} thick', this.fmt(z.thick))) : null;
    this.app.redraw();
  }
  unlabel() { const z = this.sized; if (!z) return; z.segments.forEach(s => s.el?.remove()); z.areaEl?.remove(); z.thickEl?.remove(); }
  unsize() { if (!this.sized) return; this.unlabel(); this.sized = null; this.sizedFace.visible = false; this.app.redraw(); }
  // How thick the element is behind the face hit: the nearest face of the same shape behind it, facing the other way, within 3 m.
  thickness(h) {
    const n = h.normal, from = h.point.clone().addScaledVector(n, -3), ray = new THREE.Raycaster(from, n.clone(), 0, 3 - 1e-4);
    const back = this.app.cast(ray, { only: [h.object], all: true }).filter(b => b.normal && b.normal.dot(n) < -0.999);
    if (!back.length) return null;
    const t = 3 - Math.max(...back.map(b => b.distance));
    return t > 1e-4 ? t : null;
  }

  // Each frame: the lines and points over the picture, then every label laid where it reads.
  place() {
    const r = this.app.size(), svg = [], labels = [], ink = '#1c5ca3', halo = HALO_INK;
    const line = (a, b, w, color, dash = '') => {
      const s = this.seen(a, b); if (!s) return null;
      const d = `M${s[0].x.toFixed(1)} ${s[0].y.toFixed(1)}L${s[1].x.toFixed(1)} ${s[1].y.toFixed(1)}`;
      if (w > 1.5) svg.push(`<path d="${d}" stroke="${halo}" stroke-width="${w + HALO}" stroke-linecap="round"${dash ? ` stroke-dasharray="${dash}"` : ''}/>`);
      svg.push(`<path d="${d}" stroke="${color}" stroke-width="${w}" stroke-linecap="round"${dash ? ` stroke-dasharray="${dash}"` : ''}/>`);
      return s;
    };
    // Each end in the style picked, turned along its own line (the last of `pts` towards `next`, the point being picked).
    const ends = (pts, next) => {
      const ss = (next ? [...pts, next] : pts).map(p => this.screen(p)), ws = ways(ss);
      pts.forEach((p, i) => { if (!ss[i].behind) svg.push(endMark(this.ends, ss[i].x, ss[i].y, ws[i].x, ws[i].y, MARK)); });
    };
    const size = el => { if (!el.size) { const w = el.offsetWidth, h = el.offsetHeight; if (w) el.size = { w, h }; else return { w: 60, h: 22 }; } return el.size; };
    const put = (el, at) => { el.hidden = !at; if (at) { el.style.left = at.x + 'px'; el.style.top = at.y + 'px'; } };
    const queue = (el, x, y, prio, tries, must) => { const { w, h } = size(el); labels.push({ el, x, y, w, h, prio, tries: tries(w, h), must }); };
    for (const it of this.items) {
      const on = it === this.picked;
      if (it.kind === 'angle') {
        const [a, o, c] = it.pts;
        line(a, o, 2, ink); line(o, c, 2, ink);
        for (let i = 0; i < it.arc.length - 1; i++) line(it.arc[i], it.arc[i + 1], 1.5, ink);
        ends(it.pts);
        const s = this.screen(it.at);
        if (s.behind) put(it.label, null); else queue(it.label, s.x, s.y, on ? 0 : 1, (w, h) => along(s, this.screen(o), w, h), true);
        continue;
      }
      const [a, b] = it.pts, A = a.clone().add(it.off), B = b.clone().add(it.off), pulled = it.off.lengthSq() > 1e-10;
      if (on) it.tags.forEach(t => line(t.a, t.b, LEG, AXES[t.axis][1]));
      if (pulled) { line(a, A, 1, ink); line(b, B, 1, ink); }
      const s = line(A, B, 2, ink);
      ends([a, b]);
      if (!s) put(it.label, null);
      else queue(it.label, (s[0].x + s[1].x) / 2, (s[0].y + s[1].y) / 2, on ? 0 : 1, (w, h) => along(s[0], s[1], w, h), true);
      if (on && s) {
        const mid = { x: (s[0].x + s[1].x) / 2, y: (s[0].y + s[1].y) / 2 };
        for (const t of it.tags) {
          const ts = this.seen(t.a, t.b);
          // A leg seen shorter than 24 px gets no tag: its number would hide what it names.
          if (!ts || Math.hypot(ts[1].x - ts[0].x, ts[1].y - ts[0].y) < 24) { put(t.el, null); continue; }
          queue(t.el, (ts[0].x + ts[1].x) / 2, (ts[0].y + ts[1].y) / 2, 2, (w, h) => beside(ts[0], ts[1], w, h, mid), false);
        }
      } else it.tags.forEach(t => put(t.el, null));
    }
    // The points being picked, and the line on to the cursor.
    const run = this.live ? [...this.pts, this.live] : this.pts;
    for (let i = 0; i < run.length - 1; i++) line(run[i], run[i + 1], 2, ink);
    ends(this.pts, this.live);
    // What the pointer snaps to, as its kind's shape, over everything else.
    const cs = this.cur && this.screen(this.cur.p);
    if (cs && !cs.behind) svg.push(snapMark(this.cur.kind, cs.x, cs.y, this.cur.angle, MARK));
    // A face's sizes: the longest edges first, each beside its edge; the area (and thickness) in the middle.
    const z = this.sized;
    if (z) {
      const c = this.screen(z.centre);
      if (c.behind) { put(z.areaEl, null); if (z.thickEl) put(z.thickEl, null); }
      else {
        queue(z.areaEl, c.x, c.y, 0, (w, h) => [[0, 0], [0, h + 6], [0, -h - 6]], true);
        if (z.thickEl) queue(z.thickEl, c.x, c.y + size(z.areaEl).h + 6, 1, (w, h) => [[0, 0], [0, h + 6]], false);
      }
      [...z.segments].sort((p, q) => q.a.distanceToSquared(q.b) - p.a.distanceToSquared(p.b)).forEach((g, k) => {
        const s = this.seen(g.a, g.b);
        if (!s || k >= 16 || Math.hypot(s[1].x - s[0].x, s[1].y - s[0].y) < 30) { put(g.el, null); return; }
        queue(g.el, (s[0].x + s[1].x) / 2, (s[0].y + s[1].y) / 2, 3 + k, (w, h) => beside(s[0], s[1], w, h, c), false);
      });
    }
    this.svg.innerHTML = svg.join('');
    const at = layout(labels, { x0: 0, y0: 0, x1: r.w, y1: r.h });
    labels.forEach((l, i) => put(l.el, at[i]));
  }
}
