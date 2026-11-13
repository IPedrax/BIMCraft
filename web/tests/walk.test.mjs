// Walking straight and never through a wall (`web/viewer/js/walk.js`, `body.js`), frame by frame with real wall shapes and no
// browser: three.js comes from the viewer's own copy. Run by `node --test web/tests/walk.test.mjs`.
import test from 'node:test';
import assert from 'node:assert/strict';
import { register } from 'node:module';

const THREE_URL = new URL('../viewer/vendor/three/build/three.module.js', import.meta.url).href;
register('data:text/javascript,' + encodeURIComponent(
  `export async function resolve(s, c, n) { return s === 'three' ? { url: ${JSON.stringify(THREE_URL)}, shortCircuit: true } : n(s, c); }`));
const nothing = () => ({ addEventListener() {}, getContext: () => null, getBoundingClientRect: () => ({}), width: 1, height: 1, hidden: true, style: {} });
globalThis.document = { getElementById: nothing };
globalThis.localStorage = { getItem: () => '0', setItem() {} };
const THREE = await import('three');
const { Nav } = await import('../viewer/js/nav.js');
const { Walk, RADIUS } = await import('../viewer/js/walk.js');
const { square, gapRoom } = await import('../viewer/js/body.js');

const DEG = Math.PI / 180, FPS = 1 / 60;

test('a heading within 4° of a wall is put square to it, along or across; one further off is left alone', () => {
  const wall = Math.PI / 2;   // a face pointing +x: a wall running north and south
  for (const off of [-3.9, -1, 0.5, 3.9]) {
    const y = square(off * DEG, [wall]);
    assert.ok(Math.abs(Math.sin(y * 2)) < 1e-9, `${off}° gave ${y / DEG}°`);
  }
  assert.ok(Math.abs(square(Math.PI / 2 + 2 * DEG, [wall]) - Math.PI / 2) < 1e-9);
  assert.equal(square(10 * DEG, [wall]), 10 * DEG);
  assert.equal(square(10 * DEG, []), 10 * DEG);
});

test('the room before a wall is counted square to it: met aslant, the body still stops a body width off', () => {
  assert.equal(gapRoom(1.25, 1, 0.25), 1);
  // 3° toward a wall 0.5 m to the side: the old count (along the walk, less the width) let the middle come within a centimetre.
  const cos = Math.sin(3 * DEG), along = 0.5 / cos, s = gapRoom(0.5, cos, 0.25);
  assert.ok(Math.abs((along - s) * cos - 0.25) < 1e-9);
  assert.ok(gapRoom(0.2, 0.5, 0.25) < 0);
});

// A corridor 1.6 m wide running north (−z), its walls 0.2 m thick, with a pier 0.3 m out of the east wall 6 m along.
function corridor() {
  const meshes = [];
  const box = (w, d, x, z, category = 'wall') => {
    const m = new THREE.Mesh(new THREE.BoxGeometry(w, 3, d), new THREE.MeshBasicMaterial());
    m.position.set(x, 1.5, z); m.updateMatrixWorld(true);
    const element = new THREE.Object3D(); element.visible = true;
    element.userData = { category, box: new THREE.Box3().setFromObject(m) };
    m.userData.element = element; meshes.push(m);
  };
  box(0.2, 40, -0.9, -15); box(0.2, 40, 0.9, -15); box(0.3, 0.4, 0.65, -6, 'column'); box(1.6, 0.05, 0, -10, 'door');
  return meshes;
}
function rig(meshes) {
  const ticks = [], heard = {}, cam = new THREE.PerspectiveCamera(50, 1.5, 0.05, 5000), nm = new THREE.Matrix3();
  const floor = new THREE.Mesh(new THREE.PlaneGeometry(100, 100).rotateX(-Math.PI / 2), new THREE.MeshBasicMaterial());
  floor.updateMatrixWorld(true); floor.userData.element = Object.assign(new THREE.Object3D(), { userData: { category: 'floor' } });
  const all = [...meshes, floor];
  const app = {
    eye: cam, camera: cam, parallel: false, renderer: { domElement: { style: {} } }, meshes: all, box: new THREE.Box3(),
    on: (what, f) => { if (what === 'tick') ticks.push(f); else (heard[what] ??= []).push(f); }, redraw() {}, tip() {},
    emit: (what, ...a) => { for (const f of heard[what] ?? []) f(...a); }, setParallel() {}, leaveWalk() {},
    size: () => ({ w: 1200, h: 800, left: 0, top: 0 }), free: () => ({ x0: 0, x1: 1200, y0: 0, y1: 800 }), pick: () => null,
    cast(ray, opts = {}) {
      for (const h of ray.intersectObjects(opts.only || all, false)) {
        h.normal = h.face.normal.clone().applyMatrix3(nm.getNormalMatrix(h.object.matrixWorld)).normalize();
        if (!opts.accept || opts.accept(h)) return h;
      }
      return null;
    },
  };
  const nav = new Nav(app), walk = new Walk(app, nav);
  cam.position.set(0, 1.6, 0); cam.quaternion.identity();
  walk.start();
  const frame = () => { for (const f of ticks) f(FPS); };
  return { cam, walk, frame };
}
const key = (walk, k, down) => walk.key({ key: k, preventDefault() {} }, down);
// How far the walker's middle is inside the corridor's clear width, and off the pier's faces; negative is inside a solid.
const margin = p => Math.min(0.8 - Math.abs(p.x), p.z > -6.2 - RADIUS && p.z < -5.8 + RADIUS ? 0.5 - p.x : Infinity);

test('held W, set 3° off the corridor, the walk is put square and goes straight down it, a body width off the walls', () => {
  const { walk, frame } = rig(corridor());
  walk.feet.set(-0.3, 0, 0); walk.yaw = 3 * DEG; walk.look();
  key(walk, 'w', true);
  assert.ok(Math.abs(walk.yaw) < 1e-12, `yaw ${walk.yaw}`);
  for (let i = 0; i < 60 * 8; i++) frame();
  assert.ok(Math.abs(walk.feet.x + 0.3) < 1e-6, `drifted to x ${walk.feet.x}`);
  assert.ok(walk.feet.z < -9, 'walked down the corridor, and through the door');
});

test('walked at a wall aslant for 20 s, from any angle, the body never comes nearer than its width, nor through it', () => {
  for (const deg of [-30, -12, -6, -2, 2, 6, 12, 30]) {
    const { walk, frame } = rig(corridor());
    walk.feet.set(0, 0, 0); walk.yaw = deg * DEG; walk.look();
    walk.keys.add('w');   // held from the start, as a key pressed before the heading was turned: no squaring
    let worst = Infinity;
    for (let i = 0; i < 60 * 20; i++) { frame(); worst = Math.min(worst, margin(walk.feet)); }
    assert.ok(worst >= RADIUS - 0.01, `${deg}°: came within ${worst.toFixed(3)} m`);
  }
});

test('the wheel glides as it did in the open, but a wall stops it', () => {
  const { walk, frame } = rig(corridor());
  walk.feet.set(0, 0, 0); walk.yaw = -Math.PI / 2; walk.look();   // facing east, the wall 0.8 m away
  for (let i = 0; i < 6; i++) walk.wheel({ deltaY: -100, deltaMode: 0 });
  for (let i = 0; i < 60 * 3; i++) frame();
  assert.ok(walk.feet.x <= 0.8 - RADIUS + 0.01, `glid to ${walk.feet.x}`);
});
