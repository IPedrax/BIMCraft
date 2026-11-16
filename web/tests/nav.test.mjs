// The camera's flights (`web/viewer/js/nav.js`) and Walk taking over after them (`web/viewer/js/walk.js`), frame by frame, with
// no browser: three.js comes from the viewer's own copy. Run by `node --test web/tests/nav.test.mjs`.
// What is held: no roll at any frame, the eye on the straight line, headings the short way, Go there's times, no jump when
// Walk and a flight meet, and a speed with no step in it.
import test from 'node:test';
import assert from 'node:assert/strict';
import { register } from 'node:module';

// The page's import map, for node: `three` is the vendored build.
const THREE_URL = new URL('../viewer/vendor/three/build/three.module.js', import.meta.url).href;
register('data:text/javascript,' + encodeURIComponent(
  `export async function resolve(s, c, n) { return s === 'three' ? { url: ${JSON.stringify(THREE_URL)}, shortCircuit: true } : n(s, c); }`));
// Walk asks the page for its mini plan and remembers whether it was shown; a page of nothing, with the map put away.
const nothing = () => ({ addEventListener() {}, getContext: () => null, getBoundingClientRect: () => ({}), width: 1, height: 1, hidden: true, style: {} });
globalThis.document = { getElementById: nothing };
globalThis.localStorage = { getItem: () => '0', setItem() {} };
const THREE = await import('three');
const { Nav } = await import('../viewer/js/nav.js');
const { Walk, EYE } = await import('../viewer/js/walk.js');

const DEG = 180 / Math.PI, FPS = 1 / 60;
const short = a => Math.atan2(Math.sin(a), Math.cos(a));
// A seeded draw, so a failure comes back the same.
function draws(seed) {
  let s = seed >>> 0;
  return () => { s = (s + 0x6d2b79f5) >>> 0; let t = s; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
}

// A viewer with nothing in it but a camera, ticked as the page ticks it: the flight first, then Walk.
function rig(walking = false) {
  const ticks = [], heard = {}, style = { opacity: '' };
  const cam = new THREE.PerspectiveCamera(50, 1.5, 0.05, 5000);
  const app = {
    eye: cam, camera: cam, parallel: false, renderer: { domElement: { style } }, meshes: [], box: new THREE.Box3(),
    on: (what, f) => { if (what === 'tick') ticks.push(f); else (heard[what] ??= []).push(f); }, redraw() {}, tip() {},
    emit: (what, ...a) => { for (const f of heard[what] ?? []) f(...a); }, setParallel() {}, leaveWalk() {},
    size: () => ({ w: 1200, h: 800, left: 0, top: 0 }), free: () => ({ x0: 0, x1: 1200, y0: 0, y1: 800 }),
    cast: () => null, pick: () => null,
  };
  const nav = new Nav(app);
  let clock = 1000;
  nav.now = () => clock;
  const walk = walking ? new Walk(app, nav) : null;
  const frame = (dt = FPS) => { clock += dt * 1000; for (const f of ticks) f(dt); };
  return { app, cam, nav, walk, style, frame };
}
const view = (yaw, pitch) => new THREE.Quaternion().setFromEuler(new THREE.Euler(pitch, yaw, 0, 'YXZ'));
function stand(cam, p, yaw, pitch) { cam.position.copy(p); cam.quaternion.copy(view(yaw, pitch)); }
const snap = r => ({ p: r.cam.position.clone(), q: r.cam.quaternion.clone(), opacity: r.style.opacity });
// The frames of a flight: where the camera stood before it, at each frame, and a few frames after it ended.
function frames(r, dt = FPS, after = 3) {
  const out = [snap(r)];
  for (let i = 0; i < 2000 && r.nav.fly; i++) { r.frame(dt); out.push(snap(r)); }
  assert.equal(r.nav.fly, null, 'the flight ends');
  for (let i = 0; i < after; i++) { r.frame(dt); out.push(snap(r)); }
  return out;
}
const roll = q => Math.asin(THREE.MathUtils.clamp(new THREE.Vector3(1, 0, 0).applyQuaternion(q).y, -1, 1)) * DEG;
const forward = q => new THREE.Vector3(0, 0, -1).applyQuaternion(q);
const headingOf = q => { const f = forward(q); return Math.atan2(-f.x, -f.z); };
function offLine(p, a, b) {
  const ab = b.clone().sub(a), l = ab.lengthSq();
  const t = l > 0 ? THREE.MathUtils.clamp(p.clone().sub(a).dot(ab) / l, 0, 1) : 0;
  return p.distanceTo(a.clone().addScaledVector(ab, t));
}
// A pair of level views (no roll), `far` apart or less: heading anywhere, tilt within Walk's 80°.
function pair(rand, far) {
  const at = () => (rand() * 2 - 1) * 30;
  const p0 = new THREE.Vector3(at(), at() / 3, at());
  const d = new THREE.Vector3(rand() * 2 - 1, (rand() * 2 - 1) * 0.6, rand() * 2 - 1).normalize();
  const k = rand(), len = k < 0.25 ? rand() * 0.5 : k < 0.9 ? 0.5 + rand() * 39.5 : far ? 40.5 + rand() * 160 : rand() * 40;
  const tilt = () => (rand() * 2 - 1) * 80 / DEG;
  return { p0, p1: p0.clone().addScaledVector(d, len), yaw0: (rand() * 2 - 1) * Math.PI, yaw1: (rand() * 2 - 1) * Math.PI, pitch0: tilt(), pitch1: tilt(), len };
}
// Each way the page flies: Go there's glide, and flyTo as Fit, Home and today's Go there call it.
const WAYS = {
  glide: (r, c) => r.nav.glideTo(c.p1.clone(), view(c.yaw1, c.pitch1), 5),
  flyTo: (r, c) => r.nav.flyTo(c.p1.clone(), view(c.yaw1, c.pitch1), 5, false, Math.min(2000, 700 + c.len * 30)),
};
function fly(way, c, dt = FPS, walking = false) {
  const r = rig(walking);
  stand(r.cam, c.p0, c.yaw0, c.pitch0);
  WAYS[way](r, c);
  return { r, seen: frames(r, dt) };
}

for (const way of Object.keys(WAYS)) {
  test(`${way}: no roll at any frame between two level views, over 1,000 pairs, and it ends exactly where it was sent`, t => {
    const rand = draws(way === 'glide' ? 11 : 12);
    let most = 0;
    for (let i = 0; i < 1000; i++) {
      const c = pair(rand, true), { seen } = fly(way, c);
      for (const s of seen) { most = Math.max(most, Math.abs(roll(s.q))); assert.ok(s.p.toArray().every(Number.isFinite)); }
      const end = seen[seen.length - 1];
      assert.ok(end.p.distanceTo(c.p1) < 1e-9, `ends at the eye it was sent to (${end.p.distanceTo(c.p1)} m off)`);
      assert.ok(end.q.angleTo(view(c.yaw1, c.pitch1)) < 1e-7, 'ends looking the way it was sent');
    }
    t.diagnostic(`most roll at any frame: ${most.toFixed(6)}°`);
    assert.ok(most < 0.0005, `the most roll was ${most.toFixed(6)}°`);
  });

  test(`${way}: the eye never leaves the straight line by 1 mm`, t => {
    const rand = draws(way === 'glide' ? 21 : 22);
    let most = 0;
    for (let i = 0; i < 1000; i++) {
      const c = pair(rand, false), { seen } = fly(way, c);
      for (const s of seen) most = Math.max(most, offLine(s.p, c.p0, c.p1));
    }
    t.diagnostic(`most off the line: ${(most * 1000).toFixed(6)} mm`);
    assert.ok(most <= 0.001, `the eye left the line by ${(most * 1000).toFixed(4)} mm`);
  });

  // Speed with no step in it: a step in velocity is as big at 240 frames a second as at 60, a smooth change four times smaller.
  test(`${way}: the eye's speed and the turn's speed change with no step, from rest to rest`, () => {
    const rand = draws(way === 'glide' ? 31 : 32);
    const steps = (c, dt) => {
      const { seen } = fly(way, c, dt), v = [], w = [];
      for (let i = 1; i < seen.length; i++) {
        v.push(seen[i].p.clone().sub(seen[i - 1].p).divideScalar(dt));
        w.push(forward(seen[i].q).angleTo(forward(seen[i - 1].q)) / dt);
      }
      let dv = 0, dw = 0, top = 0, turn = 0;
      for (let i = 1; i < v.length; i++) { dv = Math.max(dv, v[i].distanceTo(v[i - 1])); dw = Math.max(dw, Math.abs(w[i] - w[i - 1])); }
      for (const x of v) top = Math.max(top, x.length());
      for (const x of w) turn = Math.max(turn, x);
      return { dv, dw, top, turn, first: v[0].length(), last: v[v.length - 1].length() };
    };
    for (let i = 0; i < 200; i++) {
      const c = pair(rand, false), a = steps(c, FPS), b = steps(c, FPS / 4);
      assert.ok(b.dv <= a.dv / 2.5 + 1e-6, `pair ${i}: a step in speed (${a.dv.toFixed(3)} then ${b.dv.toFixed(3)} m/s a frame)`);
      assert.ok(b.dw <= a.dw / 2.5 + 1e-6, `pair ${i}: a step in the turn's speed (${a.dw.toFixed(3)} then ${b.dw.toFixed(3)} rad/s a frame)`);
      assert.ok(a.dv <= 0.2 * a.top + 1e-6, `pair ${i}: speed changed by ${a.dv.toFixed(3)} of ${a.top.toFixed(3)} m/s in a frame`);
      assert.ok(a.first <= 0.05 * a.top + 1e-6 && a.last <= 0.05 * a.top + 1e-6, `pair ${i}: starts and stops gently`);
    }
  });
}

test('glide: turns the short way, never over 180° in all, in 1.5 s at most and 0.4 s at least for a move over 0.5 m', t => {
  const rand = draws(41);
  let longest = 0, shortest = Infinity, widest = 0;
  for (let i = 0; i < 1000; i++) {
    const c = pair(rand, true), { seen } = fly('glide', c);
    let turned = 0, moving = 0;
    for (let j = 1; j < seen.length; j++) {
      turned += Math.abs(short(headingOf(seen[j].q) - headingOf(seen[j - 1].q)));
      if (!seen[j].p.equals(seen[j - 1].p) || !seen[j].q.equals(seen[j - 1].q) || seen[j].opacity !== seen[j - 1].opacity) moving = j;
    }
    const way = Math.abs(short(c.yaw1 - c.yaw0));
    widest = Math.max(widest, turned);
    assert.ok(turned <= Math.PI + 1e-9, `pair ${i}: turned ${(turned * DEG).toFixed(2)}°`);
    assert.ok(Math.abs(turned - way) < 1e-6, `pair ${i}: turned ${(turned * DEG).toFixed(3)}° where the short way is ${(way * DEG).toFixed(3)}°`);
    const time = moving * FPS;
    longest = Math.max(longest, time);
    if (c.len > 0.5) shortest = Math.min(shortest, time);
  }
  t.diagnostic(`most turned: ${(widest * DEG).toFixed(3)}°; longest ${longest.toFixed(3)} s; quickest over 0.5 m ${shortest.toFixed(3)} s`);
  assert.ok(longest <= 1.5 + FPS, `the longest took ${longest.toFixed(3)} s`);
  assert.ok(shortest >= 0.4 - FPS, `the quickest over 0.5 m took ${shortest.toFixed(3)} s`);
});

test('glide: turns in place first, glides with the heading held, then settles in place', () => {
  // 20 m east of an eye that looks north, to a pin that looks south by way of east: face east, glide, turn on to south.
  const c = { p0: new THREE.Vector3(0, 1.6, 0), p1: new THREE.Vector3(20, 1.6, 0), yaw0: 0, pitch0: 0, yaw1: -Math.PI * 0.999, pitch1: -0.2, len: 20 };
  const { seen } = fly('glide', c);
  const east = -Math.PI / 2, moved = s => s.p.distanceTo(c.p0) > 1e-9, there = s => s.p.distanceTo(c.p1) < 1e-9;
  for (const s of seen) {
    if (moved(s) && !there(s)) assert.ok(Math.abs(short(headingOf(s.q) - east)) < 1e-9, 'gliding, it faces where it goes');
    if (!moved(s)) assert.ok(Math.abs(short(headingOf(s.q))) <= Math.PI / 2 + 1e-9, 'it turns toward east, not round by west');
  }
  const turning = seen.filter(s => !moved(s)).length, settling = seen.filter(there).length;
  assert.ok(turning * FPS >= 0.15 && turning * FPS <= 0.3, `the turn took ${(turning * FPS).toFixed(3)} s`);
  assert.ok(settling * FPS >= 0.2, `the settle took ${(settling * FPS).toFixed(3)} s`);
});

test('glide: past 40 m it fades out, jumps while dark, fades in, and a flight cut short gives the picture back', () => {
  const c = { p0: new THREE.Vector3(0, 1.6, 0), p1: new THREE.Vector3(120, 4.6, -30), yaw0: 0.3, pitch0: 0, yaw1: -2.8, pitch1: 0.1, len: 124 };
  const { seen } = fly('glide', c);
  const jumps = [];
  for (let j = 1; j < seen.length; j++) if (seen[j].p.distanceTo(seen[j - 1].p) > 1) jumps.push(j);
  assert.equal(jumps.length, 1, 'one jump');
  const dark = s => (s.opacity === '' ? 1 : Number(s.opacity));
  assert.ok(dark(seen[jumps[0]]) <= 0.02 && dark(seen[jumps[0] - 1]) <= 0.02, 'the jump happens while the picture is dark');
  assert.ok(seen.slice(1, jumps[0]).every(s => s.p.equals(c.p0)), 'nothing moves before the jump');
  assert.equal(seen[seen.length - 1].opacity, '', 'the picture is given back');
  const time = (seen.length - 1 - 3) * FPS;
  assert.ok(time >= 0.4 - FPS && time <= 0.5, `the fade took ${time.toFixed(3)} s`);
  // Cut short half way out (a press on the ViewCube sets the flight to null): the picture comes back at once.
  const r = rig();
  stand(r.cam, c.p0, c.yaw0, c.pitch0);
  WAYS.glide(r, c);
  for (let i = 0; i < 6; i++) r.frame();
  assert.ok(Number(r.style.opacity) < 1, 'it was fading');
  r.nav.fly = null;
  assert.equal(r.style.opacity, '', 'given back');
});

test('glide: a new Go there while the picture fades gives the picture back at once', () => {
  const r = rig();
  stand(r.cam, new THREE.Vector3(0, 1.6, 0), 0, 0);
  r.nav.glideTo(new THREE.Vector3(120, 1.6, 0), view(1, 0), 5);
  for (let i = 0; i < 6; i++) r.frame();
  assert.ok(Number(r.style.opacity) < 1, 'it was fading');
  r.nav.glideTo(new THREE.Vector3(5, 1.6, 0), view(-1, 0), 5);
  r.frame();
  assert.equal(r.style.opacity, '');
});

test('glide with reduced motion: there at once, and a walker stands there even when nobody set it on the pin', () => {
  globalThis.matchMedia = () => ({ matches: true });
  try {
    const r = rig(true);
    stand(r.cam, new THREE.Vector3(0, 1.6, 0), 0, 0); r.walk.start();
    r.nav.glideTo(new THREE.Vector3(8, 3.2, -4), view(2, -0.2), 5);
    for (let i = 0; i < 5; i++) r.frame();
    assert.ok(r.cam.position.distanceTo(new THREE.Vector3(8, 3.2, -4)) < 1e-9, 'the camera stays at the pin');
    assert.ok(r.walk.feet.distanceTo(new THREE.Vector3(8, 3.2 - EYE, -4)) < 1e-9, 'the walker is under it');
    assert.ok(r.cam.quaternion.angleTo(view(2, -0.2)) < 1e-7);
  } finally { delete globalThis.matchMedia; }
});

test('glide: a move of nothing does nothing, and a turn alone turns in place', () => {
  const r = rig();
  stand(r.cam, new THREE.Vector3(1, 2, 3), 0.4, 0.1);
  r.nav.glideTo(new THREE.Vector3(1, 2, 3), view(0.4, 0.1), 5);
  assert.ok(frames(r).every(s => s.p.equals(new THREE.Vector3(1, 2, 3))));
  const t = rig();
  stand(t.cam, new THREE.Vector3(1, 2, 3), 0.4, 0.1);
  t.nav.glideTo(new THREE.Vector3(1, 2, 3), view(-2.9, -0.3), 5);
  const seen = frames(t);
  assert.ok(seen.every(s => s.p.equals(new THREE.Vector3(1, 2, 3))), 'stays put');
  assert.ok(seen[seen.length - 1].q.angleTo(view(-2.9, -0.3)) < 1e-7);
});

// Walk and a flight never fight: each frame the camera is where the flight alone puts it, and the walker takes over from there.
const course = c => fly('glide', c).seen.map(s => s.p);
function goThere(r, c, way) {
  // As a jump to a saved place in Walk does it: the walker is set on the place, then the camera flies there.
  const w = r.walk;
  w.feet.set(c.p1.x, c.p1.y - EYE, c.p1.z); w.yaw = c.yaw1; w.pitch = c.pitch1; w.vy = 0; w.lift = null;
  WAYS[way](r, c);
}
for (const way of Object.keys(WAYS)) {
  test(`${way} in Walk: every frame is the flight's own, the walker takes over at the pin's view, and a drag or a key mid-way jumps nowhere`, () => {
    const rand = draws(way === 'glide' ? 51 : 52);
    for (let i = 0; i < 100; i++) {
      const c = pair(rand, false);
      // Untouched: frame for frame the same as the flight with no walker, then still.
      const r = rig(true);
      stand(r.cam, c.p0, c.yaw0, c.pitch0); r.walk.start();
      goThere(r, c, way);
      const alone = way === 'glide' ? course(c) : fly('flyTo', c).seen.map(s => s.p), seen = frames(r, FPS, 10);
      for (let j = 0; j < seen.length; j++) assert.ok(seen[j].p.distanceTo(alone[Math.min(j, alone.length - 1)]) < 1e-9, `pair ${i} frame ${j}: the walk moved the camera`);
      const end = seen[seen.length - 1];
      assert.ok(end.q.angleTo(view(c.yaw1, c.pitch1)) < 1e-7, 'the walker looks the pin\'s way');
      assert.ok(Math.abs(r.walk.feet.y + EYE - c.p1.y) < 1e-9 && Math.abs(short(r.walk.yaw - c.yaw1)) < 1e-9, 'the walker stands at the pin');

      // A drag part way: the camera turns where it is, it does not leap to the pin.
      const d = rig(true);
      stand(d.cam, c.p0, c.yaw0, c.pitch0); d.walk.start();
      goThere(d, c, way);
      const cut = 3 + Math.floor(rand() * 20);
      for (let j = 0; j < cut && d.nav.fly; j++) d.frame();
      const before = d.cam.position.clone(), most = maxStep(alone);
      d.walk.down({ clientX: 100, clientY: 100 }); d.walk.move({ clientX: 108, clientY: 103 }); d.walk.up();
      assert.ok(d.cam.position.distanceTo(before) <= most + 1e-9, `pair ${i}: the drag jumped the camera ${d.cam.position.distanceTo(before).toFixed(3)} m`);
      for (let j = 0; j < 5; j++) { const was = d.cam.position.clone(); d.frame(); assert.ok(d.cam.position.distanceTo(was) <= most + 1e-9, `pair ${i}: a jump after the drag`); }

      // W held all the way: ignored while flying, then a walking step a frame.
      const k = rig(true);
      stand(k.cam, c.p0, c.yaw0, c.pitch0); k.walk.start();
      goThere(k, c, way);
      k.walk.keys.add('w');
      const held = frames(k, FPS, 10), stride = k.walk.speed * FPS;
      for (let j = 1; j < held.length; j++) {
        const step = held[j].p.distanceTo(held[j - 1].p), own = j < alone.length ? alone[j].distanceTo(alone[j - 1]) : 0;
        assert.ok(step <= Math.max(own, stride) + 1e-9, `pair ${i} frame ${j}: stepped ${step.toFixed(4)} m`);
      }
    }
  });
}
function maxStep(ps) { let m = 0; for (let i = 1; i < ps.length; i++) m = Math.max(m, ps[i].distanceTo(ps[i - 1])); return m; }

test('Fit, Home and a double-click in Walk still fly there, level, and end where they were sent', () => {
  const r = rig();
  r.app.box = new THREE.Box3(new THREE.Vector3(-10, 0, -8), new THREE.Vector3(10, 7, 8));
  stand(r.cam, new THREE.Vector3(3, 1.6, 2), 2.5, 0.05);
  r.nav.fit();
  let seen = frames(r);
  for (const s of seen) assert.ok(Math.abs(roll(s.q)) < 0.0005);
  const centre = new THREE.Vector3(0, 3.5, 0), f = forward(seen[seen.length - 1].q);
  const toCentre = centre.clone().sub(seen[seen.length - 1].p).normalize();
  assert.ok(f.angleTo(toCentre) < 1e-6, 'Fit looks at the middle of the model');
  assert.ok(-f.y >= Math.sin(Math.PI / 6) - 1e-9, 'from 30° above at least');

  // Straight down (a plan from above): looked at with north up, no roll either side of the turn.
  r.nav.frame(r.app.box, new THREE.Vector3(0, -1, 0));
  const q = r.nav.fly.q1.clone();
  seen = frames(r);
  assert.ok(seen.every(s => s.p.toArray().every(Number.isFinite)));
  assert.ok(seen[seen.length - 1].q.angleTo(q) < 1e-7);

  r.nav.home();
  const home = r.nav.fly.q1.clone();
  seen = frames(r);
  for (const s of seen) assert.ok(Math.abs(roll(s.q)) < 0.0005);
  assert.ok(seen[seen.length - 1].q.angleTo(home) < 1e-7, 'Home ends at the home view');

  // A double-click in Walk: onto the floor clicked, flown to at eye height, the walker standing there after.
  const w = rig(true);
  w.app.cast = () => ({ point: new THREE.Vector3(0, 0, 0) });
  stand(w.cam, new THREE.Vector3(0, 1.6, 0), 0.7, -0.1); w.walk.start();
  for (let i = 0; i < 3; i++) w.frame();
  w.app.pick = () => ({ point: new THREE.Vector3(5, 0, 3), normal: new THREE.Vector3(0, 1, 0) });
  w.walk.jump({ clientX: 0, clientY: 0 });
  seen = frames(w, FPS, 10);
  for (const s of seen) assert.ok(Math.abs(roll(s.q)) < 0.0005 && offLine(s.p, new THREE.Vector3(0, 1.6, 0), new THREE.Vector3(5, EYE, 3)) < 1e-9);
  assert.ok(seen[seen.length - 1].p.distanceTo(new THREE.Vector3(5, EYE, 3)) < 1e-9, 'at eye height over the floor clicked');
  assert.ok(w.walk.feet.distanceTo(new THREE.Vector3(5, 0, 3)) < 1e-9, 'the walker stands there');
});
