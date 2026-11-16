// Section: one plane (X, Y, Z through the middle, or a plan cut 1.2 m over the main floor) or a box of six, chosen on
// the section bar. A plane has a white arrow that slides it and two rings, red and blue, that tilt it, each shown as its
// tip until the pointer is on it; a box has a handle on each face. The handles stand in the middle of what shows of the cut
// (`place`), so a pan or a zoom carries them along it and never away. What a cut opens is filled in the cut colour (main.js,
// `cuttable`), and what is cut away can't be picked, measured or walked into.
import * as THREE from 'three';
import { boxFace, clip, handleSize, middle, planeInBox } from './cut.js';

// Each face of the box: the axis it moves along (glTF's: x east, y up, z south), and whether it is the low or the high side.
const FACES = [['x', 0], ['x', 1], ['y', 0], ['y', 1], ['z', 0], ['z', 1]];
// Each plane's first way, toward what it keeps: X keeps the west, Y the south, Z what is under it.
const NORMALS = { x: [-1, 0, 0], y: [0, 0, 1], z: [0, -1, 0], plan: [0, -1, 0] };
const UP = new THREE.Vector3(0, 1, 0);
// Half the angle of a ring's tip, the arc shown while the pointer is on no handle.
const TIP = 0.42;
// How far inside what the panels and the bar leave free a handle stands, px.
const INSET = 24;
// Rays this close to lying in a cut, the cosine of their angle to its normal, say nothing of where on it they land.
const EDGE_ON = 0.03;

export class Section {
  constructor(app) {
    this.app = app; this.mode = null; this.kind = null;
    this.bounds = new THREE.Box3(); this.at = new THREE.Vector3(); this.n = new THREE.Vector3(1, 0, 0);
    this.group = new THREE.Group(); app.overlay.add(this.group);
    // As desktop viewers draw them: a white arrow, a red and a blue ring, the handle under the pointer or in a drag yellow
    // and the others faded. `hot` is that handle's key ('slide', 'tilt0', 'tilt1', 'face0'..'face5').
    const m = (c, o = 0.95) => new THREE.MeshBasicMaterial({ color: c, depthTest: false, transparent: true, opacity: o });
    this.handleMat = m(0x1c5ca3); this.arrowMat = m(0xffffff); this.rimMat = m(0x17202b, 0.6); this.hotMat = m(0xffd400, 1);
    this.ringMats = [m(0xe0201b), m(0x1534e0)];
    this.dimMats = new Map([this.handleMat, this.arrowMat, this.rimMat, ...this.ringMats].map(x => { const d = x.clone(); d.opacity = 0.25; return [x, d]; }));
    this.hot = null;
    this.ray = new THREE.Raycaster(); this.sight = new THREE.Raycaster();
    this.shown = true; this.spots = null; // a plane's handles stand at `at`; a box's at `spots`, by face, null when out of sight
    const bar = document.getElementById('secpop'), btn = document.getElementById('b-section');
    btn.onclick = () => {
      if (app.tool === 'measure') app.setTool(null);
      bar.hidden = !bar.hidden; document.getElementById('lookpop').hidden = true; this.buttons();
    };
    bar.addEventListener('click', e => {
      const b = e.target.closest('button'); if (!b) return;
      const k = b.dataset.sec;
      if (k === 'flip') { if (this.mode && this.mode !== 'box') { this.n.negate(); this.apply(); } return; }
      if (k === 'off') this.set(null);
      else if (k === 'sel') {
        if (!app.selected) { app.tip('Select an element first.'); setTimeout(() => app.tip(null), 2000); return; }
        this.set('box', app.boxOf([app.selected]).expandByScalar(0.3), 'sel');
      } else this.set(k === 'box' ? 'box' : 'plane', null, k);
    });
    app.on('camera', () => { if (this.mode && !this.drag) { this.place(); this.draw(); } });
  }
  reset() { this.set(null); }
  set(mode, box, kind = mode) {
    const a = this.app;
    this.mode = mode; this.kind = mode ? kind : null; this.hot = null; this.shown = true; this.spots = null;
    if (mode) {
      this.bounds.copy(box || a.box).expandByScalar(box ? 0 : a.box.getSize(new THREE.Vector3()).length() * 0.01);
      this.at.copy(box ? this.bounds.getCenter(new THREE.Vector3()) : this.middle());
      if (kind === 'plan') this.at.y = this.planHeight();
      if (mode === 'plane') this.n.fromArray(NORMALS[kind]);
    }
    this.apply();
  }
  // Where a plane first cuts: the middle element each way (the median of their middles), not the middle of the box, which
  // a long plinth or a site's edge pulls aside (on one house model a cut at the box's middle left a handful of elements on one side).
  middle() {
    const xs = [], ys = [], zs = [], c = new THREE.Vector3();
    this.app.elements.forEach(n => { const b = n.userData.box; if (!b || b.isEmpty()) return; b.getCenter(c); xs.push(c.x); ys.push(c.y); zs.push(c.z); });
    if (!xs.length) return this.bounds.getCenter(c);
    const mid = s => s.sort((p, q) => p - q)[s.length >> 1];
    return c.set(mid(xs), mid(ys), mid(zs)).clamp(this.bounds.min, this.bounds.max);
  }
  // A plan cut's first height: 1.2 m over the main floor (the level most elements are on), as a floor plan is cut.
  planHeight() {
    const lv = this.app.mainLevel();
    return lv ? lv.elevation / 1000 + 1.2 : this.app.box.getCenter(new THREE.Vector3()).y;
  }
  planes() {
    const b = this.bounds, P = (x, y, z, c) => new THREE.Plane(new THREE.Vector3(x, y, z), c);
    if (this.mode === 'box') return [P(1, 0, 0, -b.min.x), P(-1, 0, 0, b.max.x), P(0, 1, 0, -b.min.y), P(0, -1, 0, b.max.y), P(0, 0, 1, -b.min.z), P(0, 0, -1, b.max.z)];
    if (this.mode === 'plane') return [new THREE.Plane().setFromNormalAndCoplanarPoint(this.n, this.at)];
    return [];
  }
  buttons() {
    document.querySelectorAll('#secpop button').forEach(b => {
      const k = b.dataset.sec;
      if (k === 'flip') b.disabled = this.mode !== 'plane';
      else if (k === 'off') b.disabled = !this.mode;
      else b.setAttribute('aria-pressed', String(k === this.kind));
    });
    document.getElementById('b-section').setAttribute('aria-pressed', String(!!this.mode || !document.getElementById('secpop').hidden));
  }
  apply() {
    const a = this.app;
    a.renderer.clippingPlanes = this.planes();
    a.cut.value = this.mode ? 1 : 0;
    this.buttons(); this.place(); this.draw(); a.emit('section'); a.redraw();
  }
  // The size of a handle where it stands, so that it looks the same at any zoom.
  scale(p) {
    const { parallel, nav } = this.app;
    return handleSize({ parallel, dist: nav.dist, depth: p.clone().sub(nav.cam.position).dot(nav.forward()) });
  }
  // The edges of what the view leaves free for handles, as planes for `clip`, or null with no room.
  edges() {
    const a = this.app, cam = a.camera, f = a.free(), { w, h } = a.size();
    if (f.x1 - f.x0 <= 2 * INSET || f.y1 - f.y0 <= 2 * INSET) return null;
    const x0 = (f.x0 + INSET) / w * 2 - 1, x1 = (f.x1 - INSET) / w * 2 - 1, y0 = 1 - (f.y1 - INSET) / h * 2, y1 = 1 - (f.y0 + INSET) / h * 2;
    cam.updateMatrixWorld();
    const free = new THREE.Matrix4().set(2 / (x1 - x0), 0, 0, -(x1 + x0) / (x1 - x0), 0, 2 / (y1 - y0), 0, -(y1 + y0) / (y1 - y0), 0, 0, 1, 0, 0, 0, 0, 1);
    return new THREE.Frustum().setFromProjectionMatrix(free.multiply(cam.projectionMatrix).multiply(cam.matrixWorldInverse)).planes
      .map(p => [p.normal.x, p.normal.y, p.normal.z, p.constant]);
  }
  // Where on the flat outline `poly`, square to `n` through `on`, a handle stands: the middle of what shows of it on screen.
  // Null when none of it shows; undefined when it is seen edge on, so nothing says where.
  spot(poly, edges, n, on) {
    const a = this.app, seen = clip(poly, edges);
    if (seen.length < 3) return null;
    const cam = a.camera, p = new THREE.Vector3(), m = middle(seen.map(c => { p.fromArray(c).project(cam); return [p.x, p.y]; }));
    if (!m) return undefined;
    const ray = a.aim(this.sight, new THREE.Vector2(m[0], m[1])).ray;
    if (Math.abs(ray.direction.dot(n)) < EDGE_ON) return undefined;
    return ray.intersectPlane(new THREE.Plane().setFromNormalAndCoplanarPoint(n, on), p) ?? undefined;
  }
  // Puts the handles where the view can reach them. A plane's `at` slides along it, which moves no cut; a plane that has left the
  // model stays where it is, so it can be brought back. A handle stands still while one is dragged.
  place() {
    if (!this.mode || this.drag) return;
    const edges = this.edges(); if (!edges) return;
    const b = this.bounds, lo = b.min.toArray(), hi = b.max.toArray();
    if (this.mode === 'plane') {
      const poly = planeInBox(lo, hi, this.n.toArray(), this.at.toArray());
      if (poly.length < 3) { this.shown = true; return; }
      const p = this.spot(poly, edges, this.n, this.at);
      if (p !== undefined) this.shown = p !== null;
      if (p) this.at.copy(p);
      return;
    }
    this.spots = FACES.map(([k, high], i) => {
      const n = new THREE.Vector3(), on = b.getCenter(new THREE.Vector3()); n[k] = 1; on[k] = high ? b.max[k] : b.min[k];
      const p = this.spot(boxFace(lo, hi, k, !!high), edges, n, on);
      return p === undefined ? this.spots?.[i] ?? on : p;
    });
  }
  // The plane's two ways across it: `u` level, `v` the other, both square to the plane's normal.
  axes(n = this.n) {
    const u = Math.abs(n.dot(UP)) > 0.999 ? new THREE.Vector3(1, 0, 0) : new THREE.Vector3().crossVectors(n, UP).normalize();
    return [u, new THREE.Vector3().crossVectors(n, u).normalize()];
  }
  // A handle's material: yellow when it is the hot one, faded when another is, else its own.
  mat(key, own) {
    const hot = this.drag?.key ?? this.hot;
    return !hot ? own : hot === key ? this.hotMat : this.dimMats.get(own);
  }
  // The handles and outline: a box's edges and a cone on each face, or a plane's sheet, its arrow and its rings.
  draw() {
    const g = this.group, keep = new Set([this.handleMat, this.arrowMat, this.rimMat, this.hotMat, ...this.ringMats, ...this.dimMats.values()]);
    g.traverse(o => { o.geometry?.dispose(); if (o.material && !keep.has(o.material)) o.material.dispose(); });
    g.clear(); this.handles = [];
    if (!this.mode || !this.shown) return;
    const b = this.bounds, hot = this.drag?.key ?? this.hot;
    if (this.mode === 'box') {
      const box = new THREE.Box3Helper(b.clone(), 0x1c5ca3); box.material.depthTest = false; box.material.transparent = true; box.material.opacity = 0.7; g.add(box);
      const c = b.getCenter(new THREE.Vector3());
      FACES.forEach(([k, hi], i) => {
        if (this.spots?.[i] === null) return;
        const p = (this.spots?.[i] ?? c).clone(); p[k] = hi ? b.max[k] : b.min[k];
        const d = new THREE.Vector3(); d[k] = hi ? 1 : -1;
        const r = this.scale(p) * 0.5, m = new THREE.Mesh(new THREE.ConeGeometry(r, r * 2.6, 20), this.mat(`face${i}`, this.handleMat));
        m.position.copy(p).addScaledVector(d, r * 1.6); m.quaternion.setFromUnitVectors(UP, d); m.renderOrder = 10;
        m.userData = { face: i, kind: 'face', key: `face${i}` }; g.add(m); this.handles.push(m);
      });
      return;
    }
    // A plane: a sheet as wide as the model where it cuts, an arrow toward what is cut away, and the two tilt rings.
    const [u, v] = this.axes(), s = b.getSize(new THREE.Vector3()).length(), q = new THREE.Quaternion().setFromRotationMatrix(new THREE.Matrix4().makeBasis(u, v, this.n));
    const sheet = new THREE.Mesh(new THREE.PlaneGeometry(s, s), new THREE.MeshBasicMaterial({ color: 0x1c5ca3, transparent: true, opacity: 0.06, side: THREE.DoubleSide, depthTest: false, depthWrite: false }));
    sheet.quaternion.copy(q); sheet.position.copy(this.at); g.add(sheet);
    // The arrow runs from the cut's middle to where the two rings cross. A ring shows only its tip there, a short arc,
    // until the pointer is on it or it is dragged; then it shows whole.
    // The arrow is white with a dark rim, drawn first and a little larger, so it reads on a white wall.
    const r = this.scale(this.at), R = r * 4, away = this.n.clone().negate(), L = R - r * 1.1, e = r * 0.08;
    const arrow = new THREE.Group(); arrow.position.copy(this.at); arrow.quaternion.setFromUnitVectors(UP, away);
    const am = this.mat('slide', this.arrowMat), rim = this.mat('slide', this.rimMat);
    const shaft = new THREE.Mesh(new THREE.CylinderGeometry(r * 0.1, r * 0.1, L, 12), am); shaft.position.y = L / 2;
    const head = new THREE.Mesh(new THREE.ConeGeometry(r * 0.34, r * 1.1, 20), am); head.position.y = R - r * 0.55;
    const shaftRim = new THREE.Mesh(new THREE.CylinderGeometry(r * 0.1 + e, r * 0.1 + e, L + e, 12), rim); shaftRim.position.y = L / 2;
    const headRim = new THREE.Mesh(new THREE.ConeGeometry(r * 0.34 + e * 1.5, r * 1.1 + e * 3, 20), rim); headRim.position.y = R - r * 0.55;
    const grip = new THREE.Mesh(new THREE.CylinderGeometry(r * 0.45, r * 0.45, R * 0.85, 8), new THREE.MeshBasicMaterial({ visible: false })); grip.position.y = R * 0.425;
    for (const m of [shaftRim, headRim, shaft, head, grip]) { m.renderOrder = m.material === rim ? 10 : 11; m.userData = { kind: 'slide', key: 'slide' }; arrow.add(m); }
    g.add(arrow); this.handles.push(grip);
    [u, v].forEach((axis, i) => {
      const key = `tilt${i}`, whole = hot === key, arc = whole ? Math.PI * 2 : TIP * 2;
      const ringQ = new THREE.Quaternion().setFromRotationMatrix(new THREE.Matrix4().makeBasis(away, new THREE.Vector3().crossVectors(axis, away), axis));
      const torus = (tube, segs) => { const t = new THREE.TorusGeometry(R, tube, 8, segs, arc); if (!whole) t.rotateZ(-TIP); return t; };
      const ring = new THREE.Mesh(torus(r * 0.12, whole ? 96 : 16), this.mat(key, this.ringMats[i]));
      const hit = new THREE.Mesh(torus(r * 0.35, whole ? 48 : 8), new THREE.MeshBasicMaterial({ visible: false }));
      for (const m of [ring, hit]) { m.position.copy(this.at); m.quaternion.copy(ringQ); m.renderOrder = whole ? 12 : 10; m.userData = { kind: 'tilt', key, axis: axis.clone() }; g.add(m); }
      this.handles.push(hit);
    });
  }
  // The handle under the pointer, or null.
  under(e) {
    if (!this.mode || !this.handles?.length) return null;
    this.group.updateMatrixWorld(true); // handles drawn since the last frame have no place in the world yet
    this.app.aim(this.ray, this.app.ndc(e.clientX, e.clientY));
    return this.ray.intersectObjects(this.handles, false)[0] ?? null;
  }
  // The pointer moved with nothing pressed: the handle under it turns yellow (a ring shows whole). True when on one.
  over(e) {
    const key = (e && this.under(e)?.object.userData.key) ?? null;
    if (key !== this.hot) { this.hot = key; this.draw(); this.app.redraw(); }
    return !!key;
  }
  // A press on a handle starts dragging it; anything else is left to the other tools.
  down(e) {
    const h = this.under(e); if (!h) return false;
    const d = h.object.userData;
    if (d.kind === 'face') {
      const [k, hi] = FACES[d.face], axis = new THREE.Vector3(); axis[k] = 1;
      this.drag = { kind: 'face', face: d.face, axis, base: hi ? this.bounds.max[k] : this.bounds.min[k], origin: h.object.position.clone() };
    } else if (d.kind === 'slide') this.drag = { kind: 'slide', axis: this.n.clone().negate(), at0: this.at.clone(), origin: this.at.clone() };
    else this.drag = { kind: 'tilt', axis: d.axis, n0: this.n.clone(), a0: this.angleOn(d.axis) };
    if (this.drag.kind !== 'tilt') this.drag.s0 = this.along(this.drag) ?? 0;
    this.drag.key = d.key; this.hot = d.key;
    this.draw(); this.app.redraw();
    return true;
  }
  // Where the mouse's ray meets the plane square to `axis` through the cut's middle, as an angle about that axis.
  angleOn(axis) {
    const p = new THREE.Plane().setFromNormalAndCoplanarPoint(axis, this.at), x = this.ray.ray.intersectPlane(p, new THREE.Vector3());
    if (!x) return null;
    const [a, b] = this.axes(axis), w = x.sub(this.at);
    return Math.atan2(w.dot(b), w.dot(a));
  }
  // Where the mouse's ray passes nearest the handle's axis: the face or the plane goes there. A ring turns the plane
  // about its axis by as much as the mouse went round it.
  move(e) {
    const d = this.drag; if (!d) return;
    this.app.aim(this.ray, this.app.ndc(e.clientX, e.clientY));
    if (d.kind === 'tilt') {
      const a = this.angleOn(d.axis); if (a === null || d.a0 === null) return;
      this.n.copy(d.n0).applyAxisAngle(d.axis, a - d.a0).normalize();
    } else {
      const s1 = this.along(d); if (s1 === null) return;
      const s = s1 - d.s0;
      if (d.kind === 'face') {
        const [k, hi] = FACES[d.face], v = d.base + s, gap = 0.05;
        if (hi) this.bounds.max[k] = Math.max(v, this.bounds.min[k] + gap); else this.bounds.min[k] = Math.min(v, this.bounds.max[k] - gap);
      } else this.at.copy(d.at0).addScaledVector(d.axis, s);
    }
    this.app.renderer.clippingPlanes = this.planes();
    this.draw(); this.app.emit('section'); this.app.redraw();
  }
  // How far along the drag's axis, from where it started, the mouse's ray passes nearest it.
  along(d) {
    const o = this.ray.ray.origin, dir = this.ray.ray.direction, w0 = d.origin.clone().sub(o);
    const b = d.axis.dot(dir), dd = d.axis.dot(w0), e2 = dir.dot(w0), den = 1 - b * b;
    return den < 1e-6 ? null : (b * e2 - dd) / den;
  }
  up() { if (this.drag) { this.drag = null; this.place(); this.draw(); this.app.redraw(); } }
}
