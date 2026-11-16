// Walk: the camera at eye height over the floor under it, as walkthroughs have it: the eye 1.6 m
// over the floor, a step up of 0.4 m (it climbs stairs and ramps), any drop come down with gravity, and the faces of every
// solid element but doors (and rooms, masses and the like, which are not solid) stopping a body 0.25 m wide, kept that far from
// a wall's face whichever way it meets it. A floor is any face flatter than 60°. A mini plan shows where the walker is, cut at
// chest height; a press or a drag on it moves the walker there.
//
// The walker goes straight: W and S keep the heading, which turns only with the arrow keys or a drag, and a heading within 4° of a
// wall nearby is put square to it as a walk begins, so a corridor walked down stays walked down (`square` in body.js).
//
// Starting moves nothing, as in Autodesk's viewer: the camera stays where it is and looks where it looked, and gravity does the
// rest. With a floor anywhere under it, it comes down onto that (the nearest, which from over a house is the roof); with none
// (a view from outside the model) it holds its height and flies on, and comes down once something is under it.
import * as THREE from 'three';
import { fall, rise, liftTo, roll, glide, square, gapRoom } from './body.js';

export const EYE = 1.6, STEP_UP = 0.4, RADIUS = 0.25, MAX_LOOK = THREE.MathUtils.degToRad(80);
// What a body walks through: doors, and what is not a solid thing at all.
const PASS = new Set(['door', 'opening', 'room', 'roomseparator', 'space', 'area', 'shaft', 'mass', 'scopebox', 'pointcloud', 'rebar',
  'line', 'modelline', 'modeltext', 'region', 'grid', 'level', 'cadimport', 'text', 'tag', 'dimension', 'spot', 'symbol']);
const NEAR = 6;   // Metres around the walker whose solids are tried, found again after it goes 2 m.
const FEELERS = 8;   // Ways round the body it is kept clear in.
const FLAT = Math.cos(THREE.MathUtils.degToRad(60));
const DOWN = new THREE.Vector3(0, -1, 0);

export class Walk {
  constructor(app, nav) {
    this.app = app; this.nav = nav;
    this.on = false; this.keys = new Set(); this.speed = 1.4; this.gravity = true; this.vy = 0;
    this.lift = null; this.rolling = 0; this.landed = false;
    this.feet = new THREE.Vector3(); this.yaw = 0; this.pitch = 0;
    this.ray = new THREE.Raycaster();
    this.map = { canvas: document.getElementById('minimap'), image: null, centre: null, floor: null, half: 12 };
    app.on('tick', dt => this.tick(dt));
    app.on('visibility', () => { if (this.on) this.solids = this.solidMeshes(); });
    app.on('flown', () => { if (this.on) this.adopt(); });
    // The mini plan is put away with its × (or M) and brought back with the map button (or M), and stays as it was left.
    try { this.mapOn = localStorage.getItem('bimcraft-viewer-map') !== '0'; } catch { this.mapOn = true; }
    document.getElementById('maphide').onclick = () => this.showMap(false);
    document.getElementById('mapshow').onclick = () => this.showMap(true);
    // On the mini plan, a press puts the walker there and a drag carries it along, facing the way it faced; the plan stays
    // put under the pointer until it is let go, then centres on the walker again.
    const c = this.map.canvas;
    c.addEventListener('pointerdown', e => {
      if (!this.on) return;
      if (!this.map.centre) this.renderMap();
      c.setPointerCapture(e.pointerId); this.map.held = true; this.place(e); e.preventDefault();
    });
    c.addEventListener('pointermove', e => { if (this.map.held) this.place(e); });
    const end = () => { if (this.map.held) { this.map.held = false; this.map.centre = null; this.drawMap(); } };
    c.addEventListener('pointerup', end); c.addEventListener('pointercancel', end);
  }
  // The walker onto the floor under a point of the mini plan, on the floor it was cut over or one below within a storey.
  place(e) {
    const M = this.map, c = M.canvas, r = c.getBoundingClientRect(), s = c.width / (2 * M.half);
    const px = (e.clientX - r.left) * c.width / r.width, py = (e.clientY - r.top) * c.height / r.height;
    const x = M.centre.x + (px - c.width / 2) / s, z = M.centre.y + (py - c.height / 2) / s;
    const y = this.floorUnder(new THREE.Vector3(x, M.floor + 1.2, z), 4);
    if (y === null) return;
    if (this.nav.fly) { this.adopt(); this.nav.fly = null; }
    this.feet.set(x, y, z); this.vy = 0; this.lift = null; this.look(); this.drawMap();
  }
  showMap(on) {
    this.mapOn = on;
    try { localStorage.setItem('bimcraft-viewer-map', on ? '1' : '0'); } catch { /* a private window keeps nothing */ }
    document.getElementById('walkinfo').hidden = !(this.on && on);
    document.getElementById('mapshow').hidden = !(this.on && !on);
    if (on) this.map.centre = null;
  }
  solidMeshes() { this.nearAt = null; return this.app.meshes.filter(m => !PASS.has(m.userData.element.userData.category)); }
  // The solids within NEAR of the walker, by their elements' boxes: a model's thousands are tried only a few at a time.
  nearSolids() {
    const f = this.feet;
    if (this.nearAt && this.nearAt.distanceToSquared(f) < 4 && this.near) return this.near;
    this.nearAt = f.clone();
    const reach = new THREE.Box3(new THREE.Vector3(f.x - NEAR, f.y - 1, f.z - NEAR), new THREE.Vector3(f.x + NEAR, f.y + EYE + 1, f.z + NEAR));
    this.near = this.solids.filter(m => { const b = m.userData.element.userData.box; return !b || b.intersectsBox(reach); });
    return this.near;
  }

  // The floor under a point: the highest face flatter than 60° below `from`, within `reach`; null where there is none.
  floorUnder(p, reach = Infinity) {
    this.ray.set(p, DOWN); this.ray.far = reach;
    const h = this.app.cast(this.ray, { accept: h => h.normal && Math.abs(h.normal.y) >= FLAT });
    return h ? h.point.y : null;
  }
  // The wall face a body going from `feet` along `dir` (unit, level) meets first, at knee, waist and head, from its middle and its
  // two sides, so a pier's corner is met by a shoulder too. Each hit carries `room`, how far the body may go before its middle is
  // RADIUS from that face's plane, square to it.
  blocked(feet, dir, len) {
    let best = null;
    const side = new THREE.Vector3(-dir.z, 0, dir.x), only = this.nearSolids(), far = len + RADIUS * 4;
    for (const z of [STEP_UP + 0.1, 1.0, EYE - 0.05]) for (const k of [0, -0.7, 0.7]) {
      this.ray.set(new THREE.Vector3(feet.x, feet.y + z, feet.z).addScaledVector(side, k * RADIUS), dir); this.ray.far = far;
      const h = this.app.cast(this.ray, { only, accept: h => h.normal && Math.abs(h.normal.y) < FLAT });
      if (!h) continue;
      const n = h.normal.clone().setY(0).normalize();
      h.flat = n.dot(dir) > 0 ? n.negate() : n;
      const gap = (feet.x - h.point.x) * h.flat.x + (feet.z - h.point.z) * h.flat.z;
      h.room = gapRoom(gap, Math.abs(h.flat.dot(dir)), RADIUS);
      if (!best || h.room < best.room) best = h;
    }
    return best && best.room < len ? best : null;
  }
  // Move the body by `m` (a level vector), sliding along a wall it meets, then out of any face it stands too near.
  go(m) {
    let free = true;
    for (let i = 0; i < 3 && m.lengthSq() > 1e-10; i++) {
      const len = m.length(), dir = m.clone().divideScalar(len), h = this.blocked(this.feet, dir, len);
      if (!h) { this.feet.add(m); break; }
      free = false;
      const room = Math.max(0, h.room);
      this.feet.addScaledVector(dir, room);
      m = dir.multiplyScalar(len - room); m.addScaledVector(h.flat, -m.dot(h.flat));
    }
    this.clear();
    return free;
  }
  // Keep the body RADIUS from every face round it: a face nearer, in any of FEELERS ways at knee, waist and head, pushes it out.
  clear() {
    const only = this.nearSolids();
    for (let pass = 0; pass < 2; pass++) {
      let moved = false;
      for (const z of [STEP_UP + 0.1, 1.0, EYE - 0.05]) for (let i = 0; i < FEELERS; i++) {
        const a = i / FEELERS * Math.PI * 2, dir = new THREE.Vector3(Math.cos(a), 0, Math.sin(a));
        this.ray.set(new THREE.Vector3(this.feet.x, this.feet.y + z, this.feet.z), dir); this.ray.far = RADIUS * 1.5;
        const h = this.app.cast(this.ray, { only, accept: h => h.normal && Math.abs(h.normal.y) < FLAT });
        if (!h) continue;
        const n = h.normal.clone().setY(0).normalize(); if (n.dot(dir) > 0) n.negate();
        const gap = h.distance * Math.abs(n.dot(dir));
        if (gap < RADIUS - 1e-4) { this.feet.addScaledVector(n, RADIUS - gap); moved = true; }
      }
      if (!moved) return;
    }
  }
  // The headings of the walls round the walker: the level way each face near it points, in FEELERS ways at waist height.
  wallsAround() {
    const out = [], only = this.nearSolids();
    for (let i = 0; i < FEELERS; i++) {
      const a = i / FEELERS * Math.PI * 2, dir = new THREE.Vector3(Math.cos(a), 0, Math.sin(a));
      this.ray.set(new THREE.Vector3(this.feet.x, this.feet.y + 1.0, this.feet.z), dir); this.ray.far = 4;
      const h = this.app.cast(this.ray, { only, accept: h => h.normal && Math.abs(h.normal.y) < FLAT });
      if (h) out.push(Math.atan2(h.normal.x, h.normal.z));
    }
    return out;
  }
  // The walker where the camera is: while a flight carries the camera (Go there, a double-click), the walker goes with it, so
  // when the flight ends or is cut short by a drag the walk goes on from that frame's view, never from where it was sent.
  adopt() {
    const cam = this.app.camera, seen = new THREE.Euler().setFromQuaternion(cam.quaternion, 'YXZ');
    this.feet.set(cam.position.x, cam.position.y - EYE, cam.position.z); this.yaw = seen.y; this.pitch = seen.x; this.vy = 0; this.lift = null;
  }
  look() {
    const cam = this.app.camera;
    cam.position.set(this.feet.x, this.feet.y + EYE, this.feet.z);
    cam.quaternion.setFromEuler(new THREE.Euler(this.pitch, this.yaw, 0, 'YXZ'));
    this.app.redraw(); this.app.emit('camera');
  }

  // As the desktop's: a parallel view turns to perspective first, and stays so.
  start() {
    this.app.setParallel(false);
    const app = this.app, cam = app.camera, eye = cam.position;
    this.nav.fly = null;
    this.solids = this.solidMeshes(); this.on = true; this.keys.clear(); this.vy = 0; this.gravity = true; this.rolling = 0; this.landed = false;
    const seen = new THREE.Euler().setFromQuaternion(cam.quaternion, 'YXZ');
    this.yaw = seen.y; this.pitch = seen.x;
    this.feet.set(eye.x, eye.y - EYE, eye.z);
    this.lift = liftTo(eye.y, this.floorUnder(eye), EYE);
    this.look();
    this.showMap(this.mapOn);
    this.map.centre = null;
  }
  stop() {
    this.on = false; this.keys.clear();
    document.getElementById('walkinfo').hidden = document.getElementById('mapshow').hidden = true; this.app.tip(null);
    this.nav.dist = 5;
  }
  down(e) { this.drag = { x: e.clientX, y: e.clientY }; if (this.nav.fly) { this.adopt(); this.nav.fly = null; } }
  move(e) {
    const d = this.drag; if (!d) return;
    // A view from above can start steeper than a walker looks: the limit gives way to it, or the first drag would snap it.
    const most = Math.max(MAX_LOOK, Math.abs(this.pitch));
    this.yaw -= (e.clientX - d.x) * 0.004; this.pitch = THREE.MathUtils.clamp(this.pitch - (e.clientY - d.y) * 0.004, -most, most);
    d.x = e.clientX; d.y = e.clientY; this.look();
  }
  up() { this.drag = null; }
  // The wheel glides level along the way the walker faces, through walls, as the desktop's does; a fall goes on under it.
  wheel(e) { this.rolling = roll(this.rolling, e); }
  // A double-click goes there: onto the floor clicked, or in front of the wall clicked.
  jump(e) {
    const h = this.app.pick(e.clientX, e.clientY); if (!h) return;
    const p = h.point.clone();
    if (!(h.normal && h.normal.y >= FLAT)) { const back = this.app.camera.position.clone().sub(p).setY(0).normalize(); p.addScaledVector(back, 0.5); }
    const y = this.floorUnder(p.clone().setY(p.y + STEP_UP)); if (y === null) return;
    this.feet.set(p.x, y, p.z); this.vy = 0; this.lift = null;
    this.nav.flyTo(new THREE.Vector3(p.x, y + EYE, p.z), this.app.camera.quaternion.clone(), 5);
  }
  key(e, down) {
    const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    if (k === 'Escape') return false;
    if (down && k === 'm' && this.onMark) { e.preventDefault(); this.onMark(); return true; }
    if (down && (k === 'n' || k === 'm')) { this.showMap(!this.mapOn); return true; }
    if (down && (k === ' ' || k === 'Spacebar')) { this.gravity = !this.gravity; this.vy = 0; e.preventDefault(); return true; }
    if (down && (k === '+' || k === '=')) { this.speed = Math.min(20, this.speed * 1.25); return true; }
    if (down && (k === '-' || k === '_')) { this.speed = Math.max(0.2, this.speed / 1.25); return true; }
    if (['w', 'a', 's', 'd', 'e', 'q', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Shift'].includes(k)) {
      // A walk begun lines up with the walls round it, when it nearly does already.
      const walking = ['w', 's', 'ArrowUp', 'ArrowDown'], still = !walking.some(w => this.keys.has(w));
      if (down && walking.includes(k) && still && !this.keys.has(k)) this.squareUp();
      if (down) this.keys.add(k); else this.keys.delete(k);
      if (down && (k === 'e' || k === 'q') && this.gravity) this.gravity = false;
      e.preventDefault(); return true;
    }
    return false;
  }
  squareUp() {
    const y = square(this.yaw, this.wallsAround());
    if (y !== this.yaw) { this.yaw = y; this.look(); }
  }
  // The floor under the body, followed: a step up is taken at once, a drop is fallen, and over nothing the height is held.
  settle(dt) {
    if (!this.gravity) return;
    if (this.lift !== null) {
      this.feet.y = rise(this.feet.y, this.lift, dt);
      if (this.feet.y >= this.lift) { this.lift = null; this.landed = true; }
      return;
    }
    const floor = this.floorUnder(this.feet.clone().setY(this.feet.y + STEP_UP));
    const s = fall(this.feet.y, floor, this.vy, dt);
    this.feet.y = s.y; this.vy = s.vy; this.landed = floor !== null && s.y === floor;
  }
  tick(dt) {
    if (!this.on) return;
    // The flight's frame is drawn first (nav ticks before Walk): the walker takes it, and once more on the frame it ends.
    if (this.nav.fly || this.carried) { this.carried = !!this.nav.fly; this.adopt(); return; }
    const k = this.keys, run = k.has('Shift') ? 3 : 1, v = this.speed * run * dt;
    if (k.has('ArrowLeft')) this.yaw += 1.6 * dt;
    if (k.has('ArrowRight')) this.yaw -= 1.6 * dt;
    const f = new THREE.Vector3(-Math.sin(this.yaw), 0, -Math.cos(this.yaw)), r = new THREE.Vector3(-f.z, 0, f.x);
    const m = new THREE.Vector3();
    if (k.has('w') || k.has('ArrowUp')) m.add(f);
    if (k.has('s') || k.has('ArrowDown')) m.sub(f);
    if (k.has('d')) m.add(r);
    if (k.has('a')) m.sub(r);
    const moving = m.lengthSq() > 0 || k.has('ArrowLeft') || k.has('ArrowRight') || k.has('e') || k.has('q') || this.vy !== 0 || this.rolling !== 0;
    if (m.lengthSq()) this.go(m.normalize().multiplyScalar(v));
    if (this.rolling) {
      const g = glide(this.rolling, dt); this.rolling = g.left;
      if (!this.go(f.clone().multiplyScalar(g.step))) this.rolling = 0;
    }
    if (!this.gravity) { if (k.has('e')) this.feet.y += v; if (k.has('q')) this.feet.y -= v; }
    const before = this.feet.y; this.settle(dt);
    if (moving || before !== this.feet.y) this.look();
    if (this.mapOn) this.drawMap();
  }

  // The mini plan: the model seen from above, cut 1.2 m over the walker's floor, around where the walker is; drawn again
  // when the walker goes far from its middle or to another floor, not all the way down a fall: that waits for the landing.
  drawMap() {
    const M = this.map, c = M.canvas, g = c.getContext('2d');
    if (!M.held && (!M.centre || (this.vy === 0 && (M.centre.distanceTo(new THREE.Vector2(this.feet.x, this.feet.z)) > M.half * 0.5 || Math.abs(M.floor - this.feet.y) > 1.5)))) this.renderMap();
    if (!M.image) return;
    g.putImageData(M.image, 0, 0);
    const s = c.width / (2 * M.half), x = c.width / 2 + (this.feet.x - M.centre.x) * s, y = c.height / 2 + (this.feet.z - M.centre.y) * s;
    g.save(); g.translate(x, y); g.rotate(-this.yaw);
    g.fillStyle = 'rgba(28,92,163,.25)'; g.beginPath(); g.moveTo(0, 0); g.arc(0, 0, 60, -Math.PI / 2 - 0.5, -Math.PI / 2 + 0.5); g.closePath(); g.fill();
    g.fillStyle = '#1c5ca3'; g.strokeStyle = '#fff'; g.lineWidth = 3; g.beginPath(); g.arc(0, 0, 9, 0, 2 * Math.PI); g.fill(); g.stroke();
    g.restore();
  }
  renderMap() {
    const M = this.map, app = this.app, R = app.renderer, size = M.canvas.width;
    M.centre = new THREE.Vector2(this.feet.x, this.feet.z); M.floor = this.feet.y;
    const cam = new THREE.OrthographicCamera(-M.half, M.half, M.half, -M.half, 0.1, 500);
    cam.position.set(this.feet.x, this.feet.y + 200, this.feet.z); cam.up.set(0, 0, -1); cam.lookAt(this.feet.x, this.feet.y, this.feet.z);
    const rt = new THREE.WebGLRenderTarget(size, size); rt.texture.colorSpace = THREE.SRGBColorSpace;
    const planes = R.clippingPlanes;
    R.clippingPlanes = [...planes, new THREE.Plane(new THREE.Vector3(0, -1, 0), this.feet.y + 1.2)];
    const cutWas = app.cut.value; app.cut.value = 1; app.sides(true);
    R.setRenderTarget(rt); R.clear(); R.render(app.scene, cam); R.setRenderTarget(null);
    R.clippingPlanes = planes; app.cut.value = cutWas; app.sides();
    const px = new Uint8Array(size * size * 4); R.readRenderTargetPixels(rt, 0, 0, size, size, px); rt.dispose();
    const img = new ImageData(size, size);
    for (let row = 0; row < size; row++) img.data.set(px.subarray((size - 1 - row) * size * 4, (size - row) * size * 4), row * size * 4);
    M.image = img; app.redraw();
  }
}
