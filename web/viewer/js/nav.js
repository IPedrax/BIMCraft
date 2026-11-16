// Moving the camera: orbit about the point under the cursor, pan that keeps it under the cursor, the wheel zooming toward
// it (and on through a wall when it is close), two fingers to pan and pinch, and the camera's flights: Fit, Home, Focus and
// the ViewCube's views. Up stays up; the camera can look straight down or up but never rolls. `cam` is the eye, in perspective
// or parallel: a parallel view (`app.parallel`) shows what `cam` sees `dist` ahead, so there `dist` is how much it shows, a zoom
// changes it, and a turn or a pan leaves it alone.
// The ViewCube speaks the desktop's angles (`azimuth`, `elevation`, degrees, in the model where x is east, y north, z up); the
// picture here has y up and -z north: `angles` and `aim` go between the two.
import * as THREE from 'three';
import { CUBE } from './cube.js';

const UP = new THREE.Vector3(0, 1, 0);
const NORTH = new THREE.Vector3(0, 0, -1);
const ease = t => t < 0.5 ? 2 * t * t : 1 - (-2 * t + 2) ** 2 / 2;
// Eased at both ends with no jolt: speed and its change both start and end at nothing.
const smooth = t => t * t * t * (t * (6 * t - 15) + 10);
const short = a => Math.atan2(Math.sin(a), Math.cos(a));
const RAD = Math.PI / 180, TURN = CUBE.turn * 1000;
// Go there, in milliseconds and metres: the turn toward the way it goes (by how far it turns), the glide (by how far it goes),
// the settle to the view (by how far it turns), at most `most` in all; moves of `near` or less glide without turning first,
// and past `far` the picture fades for `fade` each way and jumps. Turns under `still` radians are not made.
const GLIDE = { turn: [120, 250], glide: [400, 1000], settle: [150, 300], most: 1500, near: 0.5, far: 40, fade: 200, still: 1e-3 };
// One part of a flight: from one place and view to another, in `ms`.
const leg = (p0, p1, yaw0, yaw1, pitch0, pitch1, ms) => ({ p0, p1, yaw0, yaw1, pitch0, pitch1, ms });
const wrapped = a => { const r = ((a % 360) + 360) % 360; return r > 180 ? r - 360 : r; };
const toPicture = ([x, y, z]) => new THREE.Vector3(x, z, -y);
// Toward the camera, the picture's right and up in the model, as the desktop's `Camera::axes`, then in the picture.
function basis(az, el) {
  const a = az * RAD, e = el * RAD, c = Math.cos(e), s = Math.sin(e);
  return { back: toPicture([c * Math.cos(a), c * Math.sin(a), s]), right: toPicture([-Math.sin(a), Math.cos(a), 0]), up: toPicture([-s * Math.cos(a), -s * Math.sin(a), c]) };
}
const aim = (az, el) => { const b = basis(az, el); return new THREE.Quaternion().setFromRotationMatrix(new THREE.Matrix4().makeBasis(b.right, b.up, b.back)); };

export class Nav {
  constructor(app) {
    this.app = app; this.cam = app.eye;
    this.dist = 10;      // how far ahead the camera's point of interest is, m
    this.fly = null;     // a flight under way
    this.drag = null;
    this.now = () => performance.now();
    app.on('tick', () => this.tick());
  }
  forward() { return new THREE.Vector3(0, 0, -1).applyQuaternion(this.cam.quaternion); }
  right() { return new THREE.Vector3(1, 0, 0).applyQuaternion(this.cam.quaternion); }
  // The point the camera turns about when nothing is under the cursor.
  centre() { return this.cam.position.clone().addScaledVector(this.forward(), this.dist); }

  down(e, presses) {
    this.fly = null;
    if (presses.size >= 2) { this.drag = this.pinch(presses); return; }
    const pan = e.button === 1 || e.button === 2 || e.shiftKey || this.app.panMode;
    const h = this.app.pick(e.clientX, e.clientY);
    let point = h ? h.point : this.app.selected ? this.app.selected.userData.box.getCenter(new THREE.Vector3()) : this.centre();
    if (h && !this.app.parallel) this.dist = h.distance;
    this.drag = { kind: pan ? 'pan' : 'orbit', x: e.clientX, y: e.clientY, point, depth: point.clone().sub(this.cam.position).dot(this.forward()) };
  }
  move(e, presses) {
    const d = this.drag; if (!d) return;
    if (d.kind === 'pinch') return this.pinchMove(presses);
    const dx = e.clientX - d.x, dy = e.clientY - d.y; d.x = e.clientX; d.y = e.clientY;
    if (d.kind === 'orbit') this.orbit(dx, dy, d.point); else this.pan(dx, dy, Math.max(0.2, d.depth));
  }
  up(e, presses) {
    if (this.drag?.kind === 'pinch' && presses.size > 2) return;
    this.drag = null; this.app.redraw();
  }

  // Turn about `pivot`: left and right about the vertical, up and down about the camera's own horizontal axis.
  orbit(dx, dy, pivot) {
    const cam = this.cam, yaw = new THREE.Quaternion().setFromAxisAngle(UP, -dx * 0.006);
    let pitch = new THREE.Quaternion().setFromAxisAngle(this.right(), -dy * 0.006);
    const f0 = this.forward(), a0 = f0.angleTo(UP), a1 = f0.clone().applyQuaternion(pitch).angleTo(UP);
    if ((a1 < 0.002 && a1 < a0) || (a1 > Math.PI - 0.002 && a1 > a0)) pitch = new THREE.Quaternion();
    const q = yaw.multiply(pitch);
    cam.position.sub(pivot).applyQuaternion(q).add(pivot);
    cam.quaternion.premultiply(q);
    const f = this.forward(), a = f.angleTo(UP);
    if (a > 0.05 && a < Math.PI - 0.05) { cam.up.copy(UP); cam.lookAt(cam.position.clone().add(f)); }
    if (!this.app.parallel) this.dist = Math.max(0.3, pivot.clone().sub(cam.position).dot(f));
    this.app.redraw(); this.app.emit('camera');
  }
  // Slide the camera so what is at `depth` moves with the cursor.
  pan(dx, dy, depth) {
    const cam = this.cam, s = this.perPixel(this.app.parallel ? this.dist : depth);
    cam.position.addScaledVector(this.right(), -dx * s).addScaledVector(new THREE.Vector3(0, 1, 0).applyQuaternion(cam.quaternion), dy * s);
    this.app.redraw(); this.app.emit('camera');
  }
  // The wheel: rolled away from you, toward the point under the cursor, a share of the way each notch (close up, on through
  // what is there); rolled back, away from it.
  wheel(e) {
    this.fly = null;
    const notch = e.deltaMode === 1 ? e.deltaY * 33 : e.deltaY;
    this.zoomAt(e.clientX, e.clientY, Math.exp(notch * 0.0015));
  }
  zoomAt(x, y, k) {
    if (this.app.parallel) return this.zoomParallel(x, y, k);
    const ray = new THREE.Raycaster(); ray.setFromCamera(this.app.ndc(x, y), this.cam);
    const h = this.app.cast(ray, this.app.solid()), d = h ? h.distance : this.dist;
    let step = d * (1 - k);
    if (step > 0) step = Math.max(step, Math.min(0.25, this.app.all.getSize(new THREE.Vector3()).length() / 400));
    this.cam.position.addScaledVector(ray.ray.direction, step);
    this.dist = Math.max(0.3, d - step);
    this.app.redraw(); this.app.emit('camera');
  }
  // A parallel view shows more or less: what is under the cursor stays there, and the eye keeps as far from what is looked at.
  zoomParallel(x, y, k) {
    const r = this.app.size(), cam = this.cam, was = this.dist, big = this.app.all.getSize(new THREE.Vector3()).length();
    this.dist = THREE.MathUtils.clamp(was * k, 0.05, Math.max(was, big * 20));
    const dx = x - r.left - r.w / 2, dy = r.top + r.h / 2 - y, s = this.perPixel(was) - this.perPixel(this.dist);
    cam.position.addScaledVector(this.right(), dx * s).addScaledVector(new THREE.Vector3(0, 1, 0).applyQuaternion(cam.quaternion), dy * s)
      .addScaledVector(this.forward(), was - this.dist);
    this.app.redraw(); this.app.emit('camera');
  }
  // How far a pixel reaches, `depth` ahead of the eye.
  perPixel(depth) { return 2 * depth * Math.tan(THREE.MathUtils.degToRad(this.cam.fov / 2)) / this.app.size().h; }
  pinch(presses) {
    const [a, b] = [...presses.values()];
    return { kind: 'pinch', mid: [(a.cx + b.cx) / 2, (a.cy + b.cy) / 2], span: Math.hypot(a.cx - b.cx, a.cy - b.cy) };
  }
  pinchMove(presses) {
    const now = this.pinch(presses), d = this.drag;
    this.pan(now.mid[0] - d.mid[0], now.mid[1] - d.mid[1], this.dist);
    if (d.span > 10 && now.span > 10) this.zoomAt(now.mid[0], now.mid[1], d.span / now.span);
    this.drag = now;
  }

  // Flights: the camera eases to where it is going, so the eye follows what changed. It goes in a straight line and turns by
  // heading and tilt, the short way, never by rolling: Fit, Home and Focus do it all at once, in `ms`.
  flyTo(pos, quat, dist, instant, ms = TURN) {
    if (instant || !this.cam.position.toArray().every(Number.isFinite)) return this.jump(pos, quat, dist);
    const a = this.from(pos, quat, dist);
    this.go(a, [leg(a.p0, a.p1, a.yaw0, a.yaw1, a.pitch0, a.pitch1, Math.max(1, ms))]);
  }
  // Go there: turn in place toward where it goes, glide there in a straight line, then settle to the view it was sent to. The
  // turns between them are the short way from the heading it had to the one it is sent to, never past it, so no more than
  // half a turn in all; past 40 m the picture fades out, jumps and fades in, for a long glide shows nothing but a blur.
  glideTo(pos, quat, dist) {
    if (globalThis.matchMedia?.('(prefers-reduced-motion: reduce)').matches || !this.cam.position.toArray().every(Number.isFinite)) return this.jump(pos, quat, dist);
    const a = this.from(pos, quat, dist), way = a.p1.clone().sub(a.p0), len = way.length();
    if (len > GLIDE.far) return this.go(a, [{ ...leg(a.p0, a.p0, a.yaw0, a.yaw0, a.pitch0, a.pitch0, GLIDE.fade), fade: 'out' }, { ...leg(a.p1, a.p1, a.yaw1, a.yaw1, a.pitch1, a.pitch1, GLIDE.fade), fade: 'in' }]);
    // The heading it glides on: the way it goes when it goes forward, held to the short way round to the heading it is sent to.
    const arc = short(a.yaw1 - a.yaw0), level = Math.hypot(way.x, way.z);
    let yaw = a.yaw0, pitch = a.pitch0;
    if (len > GLIDE.near && level > 1e-3 * len) {
      const off = short(Math.atan2(-way.x, -way.z) - a.yaw0), inside = arc >= 0 ? off >= 0 && off <= arc : off <= 0 && off >= arc;
      const k = inside ? off / (arc || 1) : Math.abs(off) <= Math.abs(short(off - arc)) ? 0 : 1;
      yaw = a.yaw0 + arc * k;
      if (Math.cos(Math.atan2(-way.x, -way.z) - yaw) > 0) pitch = THREE.MathUtils.clamp(Math.atan2(way.y, level), Math.min(a.pitch0, a.pitch1), Math.max(a.pitch0, a.pitch1));
    }
    const turned = Math.max(Math.abs(yaw - a.yaw0), Math.abs(pitch - a.pitch0)), left = Math.max(Math.abs(a.yaw0 + arc - yaw), Math.abs(a.pitch1 - pitch));
    const turn = turned > GLIDE.still ? GLIDE.turn[0] + (GLIDE.turn[1] - GLIDE.turn[0]) * Math.min(1, turned / Math.PI) : 0;
    const settle = left > GLIDE.still ? GLIDE.settle[0] + (GLIDE.settle[1] - GLIDE.settle[0]) * Math.min(1, left / (Math.PI / 2)) : 0;
    const glide = len > 1e-6 ? Math.min(GLIDE.glide[0] + (GLIDE.glide[1] - GLIDE.glide[0]) * Math.log1p(len) / Math.log1p(GLIDE.far), GLIDE.most - turn - settle) : 0;
    const legs = [
      leg(a.p0, a.p0, a.yaw0, yaw, a.pitch0, pitch, turn),
      leg(a.p0, a.p1, yaw, yaw, pitch, pitch, glide),
      leg(a.p1, a.p1, yaw, a.yaw0 + arc, pitch, a.pitch1, settle),
    ].filter(l => l.ms > 0);
    if (!legs.length) return this.jump(pos, quat, dist);
    this.go(a, legs);
  }
  // Where a flight starts and ends, as heading, tilt and roll (three's YXZ order, as Walk's): a view the camera is sent from or
  // to with no roll in it has none at any frame.
  from(pos, quat, dist) {
    const s = new THREE.Euler().setFromQuaternion(this.cam.quaternion, 'YXZ'), e = new THREE.Euler().setFromQuaternion(quat, 'YXZ');
    const flat = r => (Math.abs(r) < 1e-9 ? 0 : r);
    return { p0: this.cam.position.clone(), p1: pos.clone(), q1: quat.clone(), yaw0: s.y, pitch0: s.x, roll0: flat(s.z), yaw1: s.y + short(e.y - s.y), pitch1: e.x, roll1: flat(e.z), d0: this.dist, dist };
  }
  go(a, legs) {
    this.fly = { ...a, t0: this.now(), legs, ms: legs.reduce((n, l) => n + l.ms, 0) };
    this.app.redraw();
  }
  // There at once; `flown` tells Walk, as the end of a flight does, so the walker stands where the camera is.
  jump(pos, quat, dist) {
    this.fly = null; this.cam.position.copy(pos); this.cam.quaternion.copy(quat); this.dist = dist; this.app.redraw(); this.app.emit('camera'); this.app.emit('flown');
  }
  // A flight cut short (a press, the wheel, Walk) gives the picture back if it had faded it.
  get fly() { return this.flying; }
  set fly(f) {
    this.flying = f;
    if (!f && this.faded) { this.faded = false; const s = this.app.renderer?.domElement?.style; if (s) s.opacity = ''; }
  }
  tick() {
    const f = this.fly; if (!f) return;
    if (f.about) {
      const t = Math.min(1, (this.now() - f.t0) / TURN), k = ease(t), az = f.a0 + f.da * k, el = f.e0 + f.de * k;
      this.cam.quaternion.copy(aim(az, el));
      this.cam.position.copy(f.about).addScaledVector(basis(az, el).back, f.dist);
      if (t >= 1) { this.fly = null; this.dist = f.dist; }
    } else {
      const e = Math.min(f.ms, Math.max(0, this.now() - f.t0)), all = smooth(e / f.ms);
      let l = f.legs[f.legs.length - 1], into = l.ms;
      for (let i = 0, gone = 0; i < f.legs.length; gone += f.legs[i].ms, i++) if (e <= gone + f.legs[i].ms) { l = f.legs[i]; into = e - gone; break; }
      const k = smooth(Math.min(1, into / l.ms));
      if (l.p0 === l.p1) this.cam.position.copy(l.p0); else this.cam.position.lerpVectors(l.p0, l.p1, k);
      this.cam.quaternion.setFromEuler(new THREE.Euler(l.pitch0 + (l.pitch1 - l.pitch0) * k, l.yaw0 + (l.yaw1 - l.yaw0) * k, f.roll0 + (f.roll1 - f.roll0) * all, 'YXZ'));
      this.dist = f.d0 + (f.dist - f.d0) * all;
      const s = this.app.renderer?.domElement?.style;
      if (l.fade && s) { s.opacity = String(l.fade === 'out' ? 1 - k : k); this.faded = true; } else if (this.faded) { this.faded = false; if (s) s.opacity = ''; }
      if (e >= f.ms) { this.cam.position.copy(f.p1); this.cam.quaternion.copy(f.q1); this.dist = f.dist; this.fly = null; }
    }
    this.app.redraw(); this.app.emit('camera');
    if (!this.fly) this.app.emit('flown');
  }
  looking(eye, target) {
    const dir = target.clone().sub(eye).normalize();
    const up = Math.abs(dir.dot(UP)) > 0.999 ? NORTH.clone().multiplyScalar(dir.y < 0 ? 1 : -1) : UP;
    return new THREE.Quaternion().setFromRotationMatrix(new THREE.Matrix4().lookAt(eye, target, up));
  }
  // The whole of `box` in view, looked at along `dir`.
  // Framed into the part of the picture the panels and the tool bar leave free, its middle on that part's middle.
  // A picture with no size yet (a tab opened behind others) is framed when it gets one (`later`), not with nothing to divide by.
  // `q`: which way up, for a view straight down or up.
  frame(box, dir, instant, q) {
    if (box.isEmpty()) return;
    const { w, h } = this.app.size();
    if (!(w > 0 && h > 0)) { this.later = () => this.frame(box, dir, true, q); return; }
    this.later = null;
    this.app.leaveWalk?.();
    const sphere = box.getBoundingSphere(new THREE.Sphere()), r = Math.max(sphere.radius, 0.4);
    const f = this.app.free();
    const t = Math.tan(THREE.MathUtils.degToRad(this.cam.fov) / 2);
    const v = Math.atan(t * (f.y1 - f.y0) / h), u = Math.atan(t * this.cam.aspect * (f.x1 - f.x0) / w);
    const dist = r / Math.sin(Math.min(v, u)) * 1.02;
    const eye = sphere.center.clone().addScaledVector(dir.clone().normalize(), -dist);
    this.settle(eye, q || this.looking(eye, sphere.center), dist, instant);
  }
  // To `eye`, with what it looks at on the middle of the free part of the picture.
  settle(eye, q, dist, instant) {
    const { w, h } = this.app.size(), f = this.app.free(), s = this.perPixel(dist), dx = (f.x0 + f.x1) / 2 - w / 2, dy = (f.y0 + f.y1) / 2 - h / 2;
    eye.addScaledVector(new THREE.Vector3(1, 0, 0).applyQuaternion(q), -dx * s).addScaledVector(new THREE.Vector3(0, 1, 0).applyQuaternion(q), dy * s);
    this.flyTo(eye, q, dist, instant);
  }
  fit() { this.frame(this.app.box, this.above()); }
  // Home: as the desktop's for this project: where the default 3D view's Set Current View as Home left it (from there at that
  // scale, or fitted from there), else the default. A pixel here reaches as far as one of the desktop's window.
  home(instant) {
    const H = this.app.homeView, [az, el] = H ? [H.azimuth, H.elevation] : CUBE.home, q = aim(az, el), back = basis(az, el).back;
    if (!H?.target) return this.frame(this.app.box, back.clone().negate(), instant, q);
    const { h } = this.app.size();
    if (!(h > 0)) { this.later = () => this.home(true); return; }
    this.later = null;
    this.app.leaveWalk?.();
    const dist = Math.max(0.05, h / (2 * H.scale * 1000 * Math.tan(THREE.MathUtils.degToRad(this.cam.fov) / 2)));
    this.settle(toPicture(H.target.map(v => v / 1000)).addScaledVector(back, dist), q, dist, instant);
  }
  focus(nodes) { this.frame(this.app.boxOf(nodes), this.above()); }
  // Fit and Focus keep the way the camera faces, looked at from 30° above at least: from eye height (after Walk, say) an
  // element is framed side on and lost behind whatever stands in front of it.
  above() {
    const f = this.forward(), h = new THREE.Vector3(f.x, 0, f.z), low = Math.PI / 6;
    if (-f.y >= Math.sin(low) || h.lengthSq() < 1e-6) return f;
    return h.normalize().multiplyScalar(Math.cos(low)).setY(-Math.sin(low));
  }
  // Where the camera looks from now, as the desktop says it. Straight down or up the picture's right still says the azimuth.
  angles() {
    const back = new THREE.Vector3(0, 0, 1).applyQuaternion(this.cam.quaternion), right = this.right();
    return [wrapped(Math.atan2(-right.x, -right.z) / RAD), Math.asin(Math.max(-1, Math.min(1, back.y))) / RAD];
  }
  // The same, or where a turn under way ends: arrow keys pressed fast add up.
  heading() { const f = this.fly; return f?.about ? [wrapped(f.a0 + f.da), f.e0 + f.de] : this.angles(); }
  // The ViewCube's turn: about what is in the middle of the picture, the short way round, as the desktop turns. The camera stays
  // as far off, so opposite faces swing round the model instead of through it.
  turn(az, el) {
    this.app.leaveWalk?.();
    const [a0, e0] = this.angles(), about = this.centre(), dist = this.dist, q = aim(az, el);
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) {
      return this.flyTo(about.clone().addScaledVector(basis(az, el).back, dist), q, dist, true);
    }
    this.fly = { t0: this.now(), a0, e0, da: wrapped(az - a0), de: el - e0, about, dist };
    this.app.redraw();
  }
}
