// The viewer's frame: the renderer, the model and its elements, picking, selection, what is hidden, and the tool bar. The
// tools are their own modules; each gets `app` and adds what it needs. The picture is drawn only when something changed
// (`app.redraw()`), so a still model costs the graphics card nothing.
import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { outsideAddresses } from './glb.js';
import { computeBoundsTree, acceleratedRaycast } from 'three-mesh-bvh';
import { Nav } from './nav.js';
import { Walk } from './walk.js';
import { Cube } from './cube.js';
import { Section } from './section.js';
import { Measure } from './measure.js';
import { bindEnds } from './ends.js';
import { Browser } from './browser.js';
import { Look } from './look.js';
import { LIGHT, SUN } from './light.js';
import { Views } from './views.js';
import { showProps } from './props.js';
import { movable, clearOf, overButton } from './panels.js';
import { makeCursors } from './cursors.js';
import { Caps } from './cap.js';
import { followFile, show, updating } from './follow.js';
import { Ring } from './ring.js';
import { Through, opacity } from './through.js';
import { t, tf } from './words.js';
import { rows, quietLabel } from './menu.js';

THREE.BufferGeometry.prototype.computeBoundsTree = computeBoundsTree;
// Each geometry gets its bounding volume tree the first time a ray asks, so picking millions of triangles stays quick and
// loading pays nothing for parts never pointed at.
THREE.Mesh.prototype.raycast = function (ray, hits) {
  const g = this.geometry;
  if (!g.boundsTree && g.index && g.index.count > 300) g.computeBoundsTree();
  return acceleratedRaycast.call(this, ray, hits);
};

const $ = id => document.getElementById(id);
const view = $('view'), canvas = $('gl');
const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, stencil: true });
renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
renderer.autoClear = false;
const scene = new THREE.Scene();
const overlay = new THREE.Scene(); // drawn after the model, never cut by a section: handles, measure lines, the selection's box
const css = name => getComputedStyle(document.documentElement).getPropertyValue(name).trim();
scene.background = new THREE.Color(css('--bg') || '#e9edf2');
// Each kind of face as bright as on the desktop (`light.js`). The floor's bounce casts no shadow and reaches only what faces down.
scene.add(new THREE.HemisphereLight(0xffffff, new THREE.Color().setRGB(LIGHT.ground, LIGHT.ground, LIGHT.ground), LIGHT.sky));
const sun = new THREE.DirectionalLight(0xffffff, LIGHT.sun); sun.position.set(...SUN); scene.add(sun);
const bounce = new THREE.DirectionalLight(0xffffff, LIGHT.bounce); bounce.position.set(0, -1, 0); scene.add(bounce);
const camera = new THREE.PerspectiveCamera(50, 1, 0.05, 5000);
// A parallel view's picture: the eye stays `camera`, which Nav moves, and this one looks the same way, as wide as `camera` sees at
// `nav.dist` ahead, so turning from one to the other keeps what is looked at the same size. Its clip box reaches behind the eye
// too: nothing in a parallel view is lost for being nearer than the eye.
const ortho = new THREE.OrthographicCamera(-1, 1, 1, -1, -1, 1), focus = { value: 10 }, ball = new THREE.Sphere();
function parallelCamera() {
  const half = nav.dist * Math.tan(THREE.MathUtils.degToRad(camera.fov) / 2);
  const reach = app.all ? app.all.getBoundingSphere(ball).radius + camera.position.distanceTo(ball.center) + 1 : 5000;
  Object.assign(ortho, { top: half, bottom: -half, left: -half * camera.aspect, right: half * camera.aspect, near: -reach, far: reach });
  ortho.position.copy(camera.position); ortho.quaternion.copy(camera.quaternion);
  ortho.updateProjectionMatrix(); ortho.updateMatrixWorld();
  focus.value = nav.dist;
  return ortho;
}

// What a section cuts open is filled: inside a cut solid only its back faces show, and they are drawn in the cut colour.
// The cut's fill is written after the picture is turned to what a screen shows (the sRGB step), so its colour is held as shown.
// While a cut is on, a back face also sits a few depth steps behind where it is, so where two elements touch face to face
// (a wall and its finish) the front face wins and the fill shows only where the cut opened a solid. That costs the card
// its early depth test, so the shader has it only while cutting (`cutting`).
const cut = { value: 0 }, cutColour = { value: new THREE.Color(0x56606c).convertLinearToSRGB() }, cutMaterials = new Set();
let cutCompiled = false;
function cuttable(m, push = m.userData.push?.value ?? 0) {
  m.userData.push = { value: push };
  m.onBeforeCompile = s => {
    s.uniforms.uCut = cut; s.uniforms.uCutColour = cutColour; s.uniforms.uPush = m.userData.push; s.uniforms.uFocus = focus;
    s.vertexShader = 'uniform float uPush;\nuniform float uFocus;\n' + s.vertexShader.replace('#include <project_vertex>',
      '#include <project_vertex>\nif (isOrthographic) mvPosition.z -= uPush * uFocus; else mvPosition.xyz *= 1.0 + uPush;\ngl_Position = projectionMatrix * mvPosition;');
    const back = cut.value > 0.5 ? '\ngl_FragDepth = gl_FrontFacing ? gl_FragCoord.z : min(1.0, gl_FragCoord.z + 2.0 * fwidth(gl_FragCoord.z) + 2.5e-7);' : '';
    s.fragmentShader = 'uniform float uCut;\nuniform vec3 uCutColour;\n' + s.fragmentShader.replace('#include <dithering_fragment>',
      '#include <dithering_fragment>\nif (uCut > 0.5 && !gl_FrontFacing) gl_FragColor = vec4(uCutColour, 1.0);' + back);
  };
  m.customProgramCacheKey = () => cut.value > 0.5 ? 'cut-on' : 'cut';
  m.side = cut.value > 0.5 ? THREE.DoubleSide : THREE.FrontSide;
  cutMaterials.add(m);
  return m;
}
// The shaders follow the cut being turned on or off.
function cutting() {
  const on = cut.value > 0.5; if (on === cutCompiled) return;
  cutCompiled = on; sides(on);
}
// Without a cut only front faces are drawn: where a back face lies on a front one (a wall's finish on its core, a lining in
// a door's opening) the two would fight over each pixel and flicker as the camera moves. A cut needs the back faces, as
// its fill.
function sides(both) {
  const side = both ? THREE.DoubleSide : THREE.FrontSide;
  cutMaterials.forEach(m => { if (m.side !== side) m.side = side; m.needsUpdate = true; });
}

const normalMatrix = new THREE.Matrix3();
const app = {
  near: 0.01,
  THREE, renderer, scene, overlay, eye: camera, view, canvas, cut,
  // What the picture is seen through; `eye` is where the camera stands, whichever it is.
  parallel: false,
  get camera() { return this.parallel ? parallelCamera() : camera; },
  setParallel(on) { if (this.parallel === !!on) return; this.parallel = !!on; this.redraw(); this.emit('camera'); },
  model: null, elements: [], byId: new Map(), byGuid: new Map(), levels: [], box: new THREE.Box3(), all: new THREE.Box3(),
  meshes: [], selected: null, tool: null, panMode: false,
  listeners: {},
  on(ev, fn) { (this.listeners[ev] ||= []).push(fn); },
  emit(ev, x) { (this.listeners[ev] || []).forEach(fn => fn(x)); },
  dirty: true,
  redraw() { this.dirty = true; },
  size() { const r = canvas.getBoundingClientRect(); return { w: r.width, h: r.height, left: r.left, top: r.top }; },
  ndc(x, y) { const r = this.size(); return new THREE.Vector2(((x - r.left) / r.w) * 2 - 1, -((y - r.top) / r.h) * 2 + 1); },
  // Where a screen point meets the model: the nearest face that is shown and not cut away, or null.
  pick(x, y, opts = {}) {
    return this.cast(this.aim(new THREE.Raycaster(), this.ndc(x, y)), this.solid(opts));
  },
  // `opts` for what is seen as solid: past what See Through made clear.
  solid(opts = {}) {
    const { accept } = opts;
    return { ...opts, accept: h => !this.clears(h.element) && (!accept || accept(h)) };
  },
  // All that stands under a screen point, nearest first, clear or not.
  under(x, y) { return this.cast(this.aim(new THREE.Raycaster(), this.ndc(x, y)), { all: true }).map(h => h.element); },
  // Points `ray` through `at` on the screen. A parallel camera's rays start level with the eye: from the back of its clip box, for
  // what is drawn behind that, which a zoom can leave a model or a section's handles.
  aim(ray, at) {
    const cam = this.camera; ray.setFromCamera(at, cam);
    if (this.parallel) ray.ray.origin.addScaledVector(ray.ray.direction, cam.near);
    return ray;
  },
  cast(ray, opts = {}) {
    const list = opts.only || this.meshes;
    const hits = ray.intersectObjects(list.filter(m => m.visible && m.userData.element.visible), false);
    const planes = renderer.clippingPlanes, all = [];
    for (const h of hits) {
      if (opts.far && h.distance > opts.far) return null;
      if (planes.some(p => p.distanceToPoint(h.point) < -1e-4)) continue;
      h.element = h.object.userData.element;
      // The face's way in the world: `face.normal` is in the shape's own frame, which its node turns, squashes and mirrors
      // (a mirror turns the corners' order, and so the way the face is wound, round).
      if (h.face) {
        const M = h.object.matrixWorld;
        h.normal = h.face.normal.clone().applyMatrix3(normalMatrix.getNormalMatrix(M)).normalize();
        if (M.determinant() < 0) h.normal.negate();
      }
      if (opts.accept && !opts.accept(h)) continue;
      if (!opts.all) return h;
      all.push(h);
    }
    return opts.all ? all : null;
  },
  // The part of the picture nothing floats over: inside the open panels, above the tool bar, in canvas pixels.
  free() {
    const c = canvas.getBoundingClientRect(), f = { x0: 0, y0: 0, x1: c.width, y1: c.height };
    const bar = document.getElementById('bar').getBoundingClientRect(); if (bar.height) f.y1 = Math.min(f.y1, bar.top - c.top - 8);
    if (window.innerWidth > 700) {
      const open = ['browser', 'views', 'props'].map(id => document.getElementById(id)).filter(p => !p.hidden).map(p => p.getBoundingClientRect());
      Object.assign(f, clearOf(open.map(r => ({ left: r.left - c.left, right: r.right - c.left })), c.width));
    }
    if (f.x1 - f.x0 < c.width * 0.3) { f.x0 = 0; f.x1 = c.width; }
    if (f.y1 - f.y0 < c.height * 0.3) { f.y0 = 0; f.y1 = c.height; }
    return f;
  },
  // The main floor: the level most elements are on.
  mainLevel() {
    const count = new Map();
    for (const n of this.elements) { const l = String(n.userData.properties?.level); count.set(l, (count.get(l) || 0) + 1); }
    return [...this.levels].sort((p, q) => (count.get(String(q.guid)) || 0) - (count.get(String(p.guid)) || 0))[0] || null;
  },
  // The boxes of shown elements: the whole model's, or a few elements'.
  boxOf(nodes) { const b = new THREE.Box3(); nodes.forEach(n => { if (n.userData.box) b.union(n.userData.box); }); return b; },
};
window.viewer = app;
const caps = new Caps(app, 0x56606c);
app.caps = caps;

const nav = new Nav(app), walk = new Walk(app, nav), cube = new Cube(app, nav), section = new Section(app), measure = new Measure(app),
  browser = new Browser(app), look = new Look(app, sun), views = new Views(app);
app.nav = nav; app.walk = walk; app.section = section; app.measure = measure; app.look = look; app.views = views;
// Measure's end style, picked in its settings and kept by the browser; the measure draws it.
bindEnds(document.getElementById('m-ends'), document.querySelector('label[for="m-ends"]'), undefined, style => { measure.ends = style; app.redraw(); });

function resize() {
  const r = view.getBoundingClientRect(); if (!r.width || !r.height) return;
  renderer.setSize(r.width, r.height, false); camera.aspect = r.width / r.height; camera.updateProjectionMatrix(); app.redraw();
  nav.later?.();
}
new ResizeObserver(resize).observe(view); resize();

let last = performance.now();
renderer.setAnimationLoop(() => {
  const now = performance.now(), dt = Math.min(0.1, (now - last) / 1000); last = now;
  app.emit('tick', dt);
  if (!app.dirty) return;
  app.dirty = false;
  draw();
});
// The near plane as far out as is safe, since the depth buffer's steps grow with the square of the distance over it:
// on a floor, nothing comes nearer than the walker's own girth; otherwise a fortieth of the way to what is looked at, as
// it stays while Walk flies or comes down from a long way off.
function fitNear() {
  const near = walk.on && walk.landed ? 0.1 : THREE.MathUtils.clamp(nav.dist / 40, app.near, 1);
  if (camera.near !== near) { camera.near = near; camera.updateProjectionMatrix(); }
}
function draw() {
  fitNear();
  renderer.clear();
  const seen = app.camera;
  renderer.render(scene, seen);
  caps.draw(seen);
  look.draw();
  const planes = renderer.clippingPlanes; renderer.clippingPlanes = [];
  renderer.clearDepth(); renderer.render(overlay, seen);
  renderer.clippingPlanes = planes;
  app.emit('drawn');
  cube.draw();
}
app.drawNow = () => { draw(); renderer.getContext().finish(); };

app.on('section', cutting);
// Shadows follow the cut too: three.js leaves the renderer's own planes out when it draws the shadow map, so each material
// carries them as well; else the part cut away still throws its shadow on the ground.
app.on('section', () => {
  const p = renderer.clippingPlanes; renderer.localClippingEnabled = p.length > 0;
  cutMaterials.forEach(m => {
    const had = m.clippingPlanes?.length || 0;
    m.clippingPlanes = p.length ? p : null; m.clipShadows = true;
    if (had !== p.length) m.needsUpdate = true;
  });
});
app.remakeMaterials = () => cutMaterials.forEach(m => { m.needsUpdate = true; });
app.sides = both => sides(both ?? cut.value > 0.5);

// Selection: the element lights up and a box is drawn round it, so a small part is found. What the pointer is over (in
// the model, or a row of the model browser) is lit blue, so a click's result is known before it is made.
const selMats = new Map(), hoverMats = new Map(), selBox = new THREE.Box3Helper(new THREE.Box3(), 0xff7a00);
selBox.visible = false; selBox.material.depthTest = false; overlay.add(selBox);
function litMat(mats, base, colour, k) {
  if (!mats.has(base)) { const m = cuttable(base.clone()); m.emissive = new THREE.Color(colour); m.emissiveIntensity = k; mats.set(base, m); }
  return mats.get(base);
}
const selMat = base => litMat(selMats, base, 0xff7a00, 0.75), hoverMat = base => litMat(hoverMats, base, 0x2f7de1, 0.45);
// See Through's copy of a material: it writes no depth, so what stands behind it is drawn whole.
const clearMats = new Map();
function clearMat(base) {
  if (!clearMats.has(base)) {
    const m = cuttable(base.clone(), base.userData.push?.value ?? 0);
    m.transparent = true; m.depthWrite = false; m.userData.clear = true; m.opacity = opacity(through.s.level, base.opacity); clearMats.set(base, m);
  }
  return clearMats.get(base);
}
// What the pointer is over is lit.
let hovered = new Set();
function paint(n) {
  const mat = n === app.selected ? selMat : hovered.has(n) ? hoverMat : app.clears(n) ? clearMat : null;
  n.traverse(o => { if (o.isMesh) { o.material = mat ? mat(o.userData.base) : o.userData.base; o.castShadow = !o.material.transparent; } });
}
app.hover = nodes => {
  const next = new Set((nodes || []).filter(n => n.visible));
  if (next.size === hovered.size && [...next].every(n => hovered.has(n))) return;
  const was = hovered; hovered = next;
  was.forEach(n => { if (!next.has(n)) paint(n); }); next.forEach(n => { if (!was.has(n)) paint(n); });
  app.redraw();
};
app.select = node => {
  const was = app.selected;
  app.selected = node || null;
  if (was) paint(was);
  if (node) { paint(node); selBox.box.copy(node.userData.box); selBox.visible = true; } else selBox.visible = false;
  $('b-hide').disabled = $('b-isolate').disabled = !node;
  showProps(app, node, $('propsbody'), $('propsname'));
  app.emit('select', node); app.redraw();
};

// Hiding: a hidden element is not drawn, picked, measured or walked into.
function hiddenCount() { return app.elements.reduce((n, e) => n + (e.visible ? 0 : 1), 0); }
app.setShown = (nodes, on) => {
  nodes.forEach(n => { n.visible = on; });
  if (!on && app.selected && !app.selected.visible) app.select(null);
  quiet();
  app.emit('visibility'); app.redraw();
};
// Isolate remembers what was hidden before it (by GUID, so a fetch of the file again keeps it) and where the camera was; its ✕ (or
// Esc) brings back just that and glides back there. Isolating again while isolated keeps the first of both.
let isolated = null;
app.isolate = (nodes, flat = false) => {
  if (!isolated) {
    const cam = app.camera;
    isolated = { hidden: new Set(app.elements.filter(e => !e.visible).map(e => e.userData.guid)),
      view: flat ? null : { pos: cam.position.clone(), quat: cam.quaternion.clone(), dist: nav.dist } };
  }
  const keep = new Set(nodes);
  app.elements.forEach(e => { e.visible = keep.has(e); });
  app.setShown([], true);
};
app.exitIsolate = () => {
  const was = isolated; if (!was) return;
  isolated = null;
  app.elements.forEach(e => { e.visible = !was.hidden.has(e.userData.guid); });
  app.setShown([], true);
  if (was.view && !views.open) nav.glideTo(was.view.pos, was.view.quat, was.view.dist);
};
app.showAll = () => { isolated = null; app.setShown(app.elements, true); };
// The quiet ✕ at the top: faint until the pointer is on it, then it says what it does. It is there only while something is
// isolated or hidden, and says which.
function quiet() {
  const label = quietLabel({ isolated: !!isolated, hidden: hiddenCount() });
  const q = $('quiet'), was = !q.hidden;
  q.hidden = !label;
  if (!label) return;
  const words = label.n ? tf(label.text, label.n) : t(label.text);
  q.querySelector('.ql').textContent = words; q.setAttribute('aria-label', words); q.title = label.key ? words + ' (Esc)' : words;
  if (!was) { q.classList.remove('in'); void q.offsetWidth; q.classList.add('in'); }
}
$('quiet').onclick = () => { if (isolated) app.exitIsolate(); else app.showAll(); canvas.focus(); };

// See Through: drawn again from its settings (`through.js`). What is clear throws no shadow.
const through = new Through(app);
app.through = () => {
  clearMats.forEach((m, base) => { m.opacity = opacity(through.s.level, base.opacity); });
  app.elements.forEach(paint); look.stale();
};

// Panels and the tool bar.
// What the panels push aside: the ViewCube for Properties, a view's title and Walk's map for those on the left. A panel
// dragged away gives the place back.
function crowd() {
  const v = $('view').getBoundingClientRect();
  for (const id of ['browser', 'views', 'props']) {
    const r = $(id).getBoundingClientRect();
    document.body.classList.toggle(id + '-open', !$(id).hidden && (id === 'props' ? r.right > v.right - 150 : r.left < v.left + 320));
  }
}
const keepers = ['browser', 'views', 'props'].map(id => movable($(id), $('view'), crowd));
new ResizeObserver(() => { keepers.forEach(k => k()); crowd(); }).observe($('view'));
function panel(name, on) {
  $(name).hidden = !on; $('b-' + name)?.setAttribute('aria-pressed', String(on));
  keepers.forEach(k => k()); crowd();
  // One panel at a time on a phone; on a wider screen the model browser and the views share the left side.
  const others = window.innerWidth <= 700 ? ['browser', 'props', 'views'] : name === 'props' ? [] : ['browser', 'views'];
  if (on) others.filter(o => o !== name && !$(o).hidden).forEach(o => panel(o, false));
}
app.panel = panel;
document.querySelectorAll('.panel .x').forEach(b => b.onclick = () => panel(b.closest('.panel').id, false));
$('b-browser').onclick = () => panel('browser', $('browser').hidden);
$('b-views').onclick = () => panel('views', $('views').hidden);
$('b-fit').onclick = () => views.open ? views.fit() : nav.fit();
// The Visibility fly-out: over its button, and closed by a choice, its button, Esc or Look or Section opening.
function fly(on) {
  const pop = $('vispop'), b = $('b-vis');
  pop.hidden = !on; b.setAttribute('aria-pressed', String(on));
  if (!on) return;
  $('lookpop').hidden = true; $('secpop').hidden = true;
  const v = $('view').getBoundingClientRect(), r = b.getBoundingClientRect();
  pop.style.left = overButton(r.left + r.width / 2 - v.left, pop.getBoundingClientRect().width, v.width) + 'px';
}
$('b-vis').onclick = () => fly($('vispop').hidden);
$('b-look').addEventListener('click', () => fly(false));
$('b-section').addEventListener('click', () => fly(false));
$('b-hide').onclick = () => { fly(false); if (app.selected) app.setShown([app.selected], false); };
$('b-isolate').onclick = () => { fly(false); if (app.selected) { const flat = !!views.open; app.isolate([app.selected], flat); if (!flat) nav.focus([app.selected]); } };
$('b-full').onclick = () => { if (document.fullscreenElement) document.exitFullscreen(); else document.documentElement.requestFullscreen?.().catch(() => {}); };
document.addEventListener('fullscreenchange', () => $('b-full').setAttribute('aria-pressed', String(!!document.fullscreenElement)));

// One tool at a time: pan, walk and measure take the left button's drags (pan slides the view, as the middle button does),
// and under pan and walk a click still selects and a right click still opens the menu; section's handles take their own
// drags under any tool.
app.setTool = name => {
  const was = app.tool;
  if (was === name) name = null;
  if (was === 'walk') walk.stop();
  // In a plan, a section or an elevation, Measure measures the drawing.
  const flat = !!views.open;
  if (was === 'measure') flat ? views.measureStop() : measure.stop();
  app.tool = name; app.panMode = name === 'pan';
  $('b-pan').setAttribute('aria-pressed', String(name === 'pan'));
  $('b-walk').setAttribute('aria-pressed', String(name === 'walk'));
  $('b-measure').setAttribute('aria-pressed', String(name === 'measure'));
  if (name === 'walk') walk.start();
  if (name === 'measure') { $('secpop').hidden = true; flat ? views.measureStart() : measure.start(); }
  app.emit('tool', name);
  app.hover(null); cursor(); app.redraw();
};
// Home, Fit, Focus and the ViewCube move the camera away from the walker, so they end Walk: else the next drag or key
// would put the camera back at the walker's eyes.
app.leaveWalk = () => { if (app.tool === 'walk') app.setTool(null); };
$('b-pan').onclick = () => app.setTool('pan');
$('b-walk').onclick = () => app.setTool('walk');
$('b-measure').onclick = () => app.setTool('measure');
app.tip = text => { $('tip').hidden = !text; $('tip').textContent = text || ''; };

// The cursor: what a drag does now (orbit, pan, zoom for a moment after the wheel), or what the tool does; in Walk, always the sight.
const cursors = makeCursors();
let zoomTimer = 0;
function cursor() {
  const p = [...presses.values()][0], kind = nav.drag?.kind;
  if (p?.by === section || (!p && section.hot)) { canvas.style.cursor = p ? 'grabbing' : 'grab'; return; }
  if (app.tool === 'measure') { canvas.style.cursor = 'crosshair'; return; }
  if (app.tool === 'walk') { canvas.style.cursor = cursors.walk; return; }
  canvas.style.cursor = p?.by === nav ? cursors[kind === 'orbit' ? 'orbit' : 'pan']
    : zoomTimer ? cursors.zoom
    : cursors[app.tool === 'pan' ? 'pan' : 'orbit'];
}
// What is under the pointer, found once a frame at most while nothing is pressed.
let hoverAt = null;
app.on('tick', () => {
  if (!hoverAt) return;
  const e = hoverAt; hoverAt = null;
  if (presses.size || nav.fly || (app.tool && app.tool !== 'pan') || views.open) return app.hover(null);
  const h = app.pick(e.clientX, e.clientY);
  app.hover(h ? [h.element] : null);
});

// The mouse on the model: a press without a drag is a click (select, or the tool's), a drag moves the camera, and a right
// click opens the menu of what is under it.
const presses = new Map();
const HOLD = 500;   // How long a finger stays still to open the menu, in milliseconds.
canvas.addEventListener('pointerdown', e => {
  canvas.focus(); closeMenu();
  if (!app.model) return;
  canvas.setPointerCapture(e.pointerId);
  presses.set(e.pointerId, { x: e.clientX, y: e.clientY, cx: e.clientX, cy: e.clientY, moved: false, by: null });
  const p = presses.get(e.pointerId);
  app.hover(null);
  // A finger has no right button: held still, it opens the same menu.
  if (e.pointerType === 'touch') p.hold = setTimeout(() => { if (!p.moved && presses.size === 1 && app.tool !== 'measure') { p.held = true; openMenu(e); } }, HOLD);
  if (presses.size === 1 && e.button === 0 && section.down(e)) p.by = section;
  else if (app.tool === 'measure' && !views.open && presses.size === 1 && e.button === 0 && measure.down(e)) p.by = measure;
  else if (app.tool === 'walk' && presses.size === 1) { p.by = walk; walk.down(e); }
  else { p.by = nav; nav.down(e, presses); }
  cursor();
});
canvas.addEventListener('pointermove', e => {
  const p = presses.get(e.pointerId);
  if (!p) {
    // A section handle under the pointer is lit instead of the model.
    const was = section.hot;
    if (section.over(views.open ? null : e)) { hoverAt = null; app.hover(null); }
    else if (app.tool === 'measure') measure.hover(e); else hoverAt = e;
    if (section.hot !== was) cursor();
    return;
  }
  p.cx = e.clientX; p.cy = e.clientY;
  if (Math.hypot(e.clientX - p.x, e.clientY - p.y) > 4) p.moved = true;
  p.by?.move(e, presses);
});
canvas.addEventListener('pointerleave', () => { hoverAt = null; if (!presses.size) { app.hover(null); section.over(null); cursor(); } });
function up(e) {
  const p = presses.get(e.pointerId); if (!p) return;
  p.by?.up(e, presses);
  clearTimeout(p.hold);
  presses.delete(e.pointerId);
  cursor();
  if (p.moved || p.held || p.by === section) return;
  if (e.button === 0) click(e);
  else if (e.button === 2 && app.tool !== 'measure') openMenu(e);
}
canvas.addEventListener('pointerup', up);
canvas.addEventListener('pointercancel', e => { clearTimeout(presses.get(e.pointerId)?.hold); presses.delete(e.pointerId); cursor(); });
canvas.addEventListener('wheel', e => {
  e.preventDefault(); closeMenu(); app.hover(null);
  if (app.tool === 'walk') walk.wheel(e); else nav.wheel(e);
  clearTimeout(zoomTimer); zoomTimer = setTimeout(() => { zoomTimer = 0; cursor(); }, 350); cursor();
}, { passive: false });

// The right-click menu: what can be done to the element under the pointer, and to the model.
const menu = $('ctx');
const more = $('ctxsub');
function closeMenu() { menu.hidden = more.hidden = true; }
app.closeMenu = closeMenu;
// A row's further menu beside it. Its rows are `{ label, on, off, act }`, null for a line: `on` is ticked, `off` greyed, and it
// stays open, so several can be chosen.
function openMore(row, rows) {
  const draw = () => more.replaceChildren(...rows().map(r => {
    if (!r) return document.createElement('hr');
    const b = document.createElement('button'); b.type = 'button'; b.textContent = r.label; b.disabled = !!r.off;
    b.setAttribute('role', 'menuitemcheckbox'); b.setAttribute('aria-checked', String(!!r.on));
    b.onclick = () => { r.act(); draw(); };
    return b;
  }));
  draw(); more.hidden = false;
  const v = view.getBoundingClientRect(), m = menu.getBoundingClientRect(), r = row.getBoundingClientRect(), s = more.getBoundingClientRect();
  const right = m.right - v.left + 2;
  more.style.left = (right + s.width + 6 > v.width ? Math.max(6, m.left - v.left - s.width - 2) : right) + 'px';
  more.style.top = Math.max(6, Math.min(r.top - v.top - 5, v.height - s.height - 6)) + 'px';
}
// The menu's rows at a point of the page: [label, what it does, greyed?, a further menu's rows], null for a line.
app.openMenuAt = function (x, y, items) {
  menu.replaceChildren(...items.map(it => {
    if (!it) { const s = document.createElement('hr'); return s; }
    const b = document.createElement('button'); b.type = 'button'; b.textContent = it[0]; b.disabled = !!it[2];
    if (it[3]) {
      b.className = 'more'; b.onclick = b.onmouseenter = () => openMore(b, it[3]);
    } else {
      b.onclick = () => { closeMenu(); it[1](); }; b.onmouseenter = () => { more.hidden = true; };
    }
    return b;
  }));
  more.hidden = true; menu.hidden = false;
  const v = view.getBoundingClientRect(), m = menu.getBoundingClientRect();
  menu.style.left = Math.max(6, Math.min(x - v.left, v.width - m.width - 6)) + 'px';
  menu.style.top = Math.max(6, Math.min(y - v.top, v.height - m.height - 6)) + 'px';
};
// The right-click menu on `n`, what is under the pointer, or null. A row for an element goes on `n` or else the selection.
function menuOn(n, x, y, plan) {
  const target = n || app.selected;
  const act = {
    Select: ['Select', () => app.select(n)],
    Properties: ['Properties', () => { app.select(target); panel('props', true); }],
    Focus: ['Focus', () => { app.select(target); nav.focus([target]); }],
    Hide: ['Hide', () => app.setShown([target], false)],
    Isolate: ['Isolate', () => { app.select(target); app.isolate([target], plan); if (!plan) nav.focus([target]); }],
    Transparency: ['Transparency', null, false, () => through.rows(app.selected || n)],
    'Exit isolate': [t('Exit isolate'), () => app.exitIsolate()],
    'Show hidden': [tf('Show {} hidden', hiddenCount()), () => app.showAll()],
    'Clear selection': ['Clear selection', () => app.select(null)],
    'Home view': ['Home view', () => nav.home()],
  };
  const list = rows({ picked: !!n, selected: !!app.selected, plan, isolated: !!isolated, hidden: hiddenCount() });
  if (n && !plan) app.hover([n]);
  app.openMenuAt(x, y, list.map(r => r && act[r]));
}
function openMenu(e) { menuOn(app.pick(e.clientX, e.clientY)?.element, e.clientX, e.clientY, false); }
app.openPlanMenu = (n, x, y) => menuOn(n, x, y, true);
window.addEventListener('pointerdown', e => { if (!menu.hidden && !menu.contains(e.target) && !more.contains(e.target)) closeMenu(); }, true);
canvas.addEventListener('dblclick', e => {
  // Each click of it selected what it landed on: going there is not picking that.
  if (app.tool === 'walk') { app.select(null); return walk.jump(e); }
  // In Measure a double-click on a face shows its sizes (`measure.js`).
  if (app.tool === 'measure' && !views.open) return measure.sizes(e);
  const h = app.pick(e.clientX, e.clientY);
  if (h) { app.select(h.element); nav.focus([h.element]); } else nav.fit();
});
canvas.addEventListener('contextmenu', e => e.preventDefault());
function click(e) {
  if (app.tool === 'measure') return measure.click(e);
  const h = app.pick(e.clientX, e.clientY);
  app.select(h ? h.element : null);
}
window.addEventListener('keydown', e => {
  if (e.target.closest?.('input,textarea')) return;
  if (e.key === 'Escape' && !menu.hidden) return closeMenu();
  if (e.key === 'Escape' && !$('vispop').hidden) return fly(false);
  if (app.tool === 'walk' && walk.key(e, true)) return;
  if (app.tool === 'measure' && !views.open && measure.key(e)) return e.preventDefault();
  if (!views.open && through.key(e)) return e.preventDefault();
  if (e.key === 'Escape') { if (app.tool) app.setTool(null); else if (isolated) app.exitIsolate(); else app.select(null); }
  else if (views.open && 'fFhH'.includes(e.key)) views.fit();
  else if (e.key === 'f' || e.key === 'F') app.selected ? nav.focus([app.selected]) : nav.fit();
  else if (e.key === 'h' || e.key === 'H') nav.home();
  else if (!views.open && !app.tool && cube.key(e)) return;
  else if (e.key === 'Delete' && app.selected) app.setShown([app.selected], false);
});
window.addEventListener('keyup', e => { if (app.tool === 'walk') walk.key(e, false); });

// Loading: the download with its progress, then the model and its elements.
async function fetchWithProgress(url) {
  const r = await fetch(url); if (!r.ok) throw new Error(r.status + ' ' + r.statusText);
  const total = +r.headers.get('content-length') || 0;
  if (!r.body || !total) return r.arrayBuffer();
  const out = new Uint8Array(total), reader = r.body.getReader(); let at = 0;
  for (;;) {
    const { done, value } = await reader.read(); if (done) break;
    out.set(value, at); at += value.length;
    ring.at('receiving', at / total);
  }
  return out.buffer;
}
// An element's node made ready: its box, its materials cuttable (and pushed back, for what lies behind others), and its meshes
// listed; the triangles it has.
function adopt(n, tris = { n: 0 }) {
  // From the corners themselves: a shape is stored once in a frame of its own and turned into place, so the box of
  // that turned frame would be larger than the element.
  n.userData.box = new THREE.Box3().setFromObject(n, true);
  n.traverse(o => {
    if (!o.isMesh) return;
    if (!app.mats.has(o.material)) { o.material.vertexColors = true; app.mats.set(o.material, cuttable(o.material)); }
    const steps = BEHIND[n.userData.category];
    if (steps) {
      const key = o.material.uuid + steps;
      if (!app.behind.has(key)) app.behind.set(key, cuttable(o.material.clone(), steps * PUSH));
      o.material = app.behind.get(key);
    }
    o.userData.base = o.material; o.userData.element = n; app.meshes.push(o);
    tris.n += (o.geometry.index ? o.geometry.index.count : o.geometry.attributes.position.count) / 3;
  });
  if (app.clears(n)) paint(n);
  return tris;
}
// The project's Home as the file says it, in degrees, mm and pixels to the mm; what doesn't read as one is left out.
function savedHome(h) {
  const n = v => typeof v === 'number' && Number.isFinite(v);
  if (!n(h?.azimuth) || !n(h?.elevation) || Math.abs(h.elevation) > 90) return null;
  const placed = Array.isArray(h.target) && h.target.length === 3 && h.target.every(n) && n(h.scale) && h.scale > 0;
  return { azimuth: h.azimuth, elevation: h.elevation, ...(placed ? { target: h.target, scale: h.scale } : {}) };
}
// `keep`: the same model again (a followed file that changed), so the camera, the section and what is hidden stay.
async function load(buffer, label, t0, keep = false) {
  const t1 = performance.now();
  if (!keep) { ring.at('reading', 0); await new Promise(r => setTimeout(r, 30)); }
  const outside = outsideAddresses(buffer);
  if (outside.length) throw new Error(`refused: The file names ${outside.length === 1 ? 'a part' : outside.length + ' parts'} at another address (${outside[0]}), which the viewer does not fetch. Save it again from BIMCraft.`);
  const gltf = await new GLTFLoader().parseAsync(buffer, '');
  const t2 = performance.now();
  const hidden = keep ? new Set(app.elements.filter(n => !n.visible).map(n => n.userData.guid)) : new Set();
  if (!keep) isolated = null;
  const was = keep ? app.selected?.userData.guid : null;
  if (app.model) scene.remove(app.model);
  cutMaterials.clear(); selMats.clear(); hoverMats.clear(); clearMats.clear(); hovered = new Set();
  app.model = gltf.scene; scene.add(app.model);
  app.model.updateMatrixWorld(true);
  const root = app.model.children[0] || app.model;
  const tris = { n: 0 };
  app.elements = root.children; app.byId = new Map(); app.byGuid = new Map(); app.meshes = []; app.mats = new Map(); app.behind = new Map();
  // A first load is counted on the ring as it places the elements, and shows none of them until all are placed.
  if (!keep) { ring.at('placing', 0); app.model.visible = false; }
  for (let i = 0; i < app.elements.length; i++) {
    const n = app.elements[i];
    app.byId.set(n.userData.id, n); if (n.userData.guid) app.byGuid.set(n.userData.guid, n);
    if (hidden.has(n.userData.guid)) n.visible = false;
    adopt(n, tris);
    if (!keep && i % 250 === 249) { ring.at('placing', (i + 1) / app.elements.length); await new Promise(r => setTimeout(r, 0)); }
  }
  app.model.visible = true;
  facesOut(app.meshes);
  app.levels = gltf.parser.json.scenes?.[0]?.extras?.levels || [];
  app.parser = gltf.parser;
  app.all = app.boxOf(app.elements);
  app.box = coreBox(app.elements, app.all);
  app.title = root.name || label.split('/').pop().replace(/\.glb$/i, '');
  app.near = camera.near = Math.max(0.01, app.all.getSize(new THREE.Vector3()).length() / 20000); camera.far = app.all.getSize(new THREE.Vector3()).length() * 10 + 100;
  camera.updateProjectionMatrix();
  browser.build(); views.build(gltf.parser.json.scenes?.[0]?.extras?.views);
  if (!keep) {
    const shown = gltf.parser.json.scenes?.[0]?.extras?.view_3d;
    app.parallel = shown?.parallel === true; app.homeView = savedHome(shown?.home);
    section.reset(); nav.home(true);
  }
  app.select(keep && was ? app.byGuid.get(was) || null : null);
  app.drawNow();
  const t3 = performance.now();
  $('drop').hidden = true; ring.stop(); document.body.classList.remove('empty');
  document.title = (root.name || label) + ' · BIMCraft Viewer';
  cursor();
  const mb = (buffer.byteLength / 1048576).toFixed(1);
  window.viewerStats = { ...window.viewerStats, file: label, mb: +mb, elements: app.elements.length, triangles: tris.n, fetch_ms: Math.round(t1 - t0), parse_ms: Math.round(t2 - t1), first_frame_ms: Math.round(t3 - t2), total_ms: Math.round(t3 - t0) };
  app.emit('load');
  buildTrees();
}
// Beams and columns, then walls and ceilings, then floors and roofs are drawn a little further off than they are. What
// lies on a face (a lining in a door's opening, a door's head under a lintel, a cabinet against a column, a ceiling hung
// on a slab's underside) is often closer than the depth buffer can tell apart, and the two would fight in blocks that
// change as the camera moves; this way the thing on the face always wins. Each corner is moved along the line from the
// eye, a share of its distance (`uPush`), so the face covers the same pixels and only its depth changes: 0.2 mm a step
// at 1 m, 4 mm at 20 m, never enough to bring a cabinet behind a wall through it, as a push of a fixed number of depth
// steps does far off (it grows with the square of the distance).
const BEHIND = { beam: 1, column: 1, structuralcolumn: 1, footing: 1, stripfooting: 1, wall: 2, ceiling: 2, floor: 3, roof: 3, toposolid: 3 };
const PUSH = 2e-4;
// A triangle wound against its own normal would vanish from the side it faces once back faces are not drawn: turned round.
// Each shape is checked once, where it stands (its node turns and squashes it, and mirrors some copies).
function facesOut(meshes) {
  const done = new Set(), a = new THREE.Vector3(), b = new THREE.Vector3(), c = new THREE.Vector3(), n = new THREE.Vector3(), m3 = new THREE.Matrix3();
  for (const mesh of meshes) {
    const g = mesh.geometry, idx = g.index, p = g.attributes.position, nm = g.attributes.normal;
    if (done.has(g) || !idx || !nm) continue;
    done.add(g); m3.getNormalMatrix(mesh.matrixWorld);
    const M = mesh.matrixWorld, flip = M.determinant() < 0 ? -1 : 1;
    let turned = 0;
    for (let i = 0; i < idx.count; i += 3) {
      const i0 = idx.getX(i), i1 = idx.getX(i + 1), i2 = idx.getX(i + 2);
      a.fromBufferAttribute(p, i0).applyMatrix4(M); b.fromBufferAttribute(p, i1).applyMatrix4(M); c.fromBufferAttribute(p, i2).applyMatrix4(M);
      b.sub(a); c.sub(a); b.cross(c);
      n.fromBufferAttribute(nm, i0).add(a.fromBufferAttribute(nm, i1)).add(a.fromBufferAttribute(nm, i2)).applyMatrix3(m3);
      if (flip * b.dot(n) < 0) { idx.setX(i + 1, i2); idx.setX(i + 2, i1); turned++; }
    }
    if (turned) idx.needsUpdate = true;
  }
}
// The picking trees, built a few at a time while nothing else is going on, so the first click or walk doesn't wait for them.
function buildTrees() {
  const todo = app.meshes.filter(m => m.geometry.index && m.geometry.index.count > 300 && !m.geometry.boundsTree);
  const step = () => {
    const t0 = performance.now();
    while (todo.length && performance.now() - t0 < 25) { const g = todo.pop().geometry; if (!g.boundsTree) g.computeBoundsTree(); }
    if (todo.length) setTimeout(step, 15);
  };
  setTimeout(step, 200);
}
// The box where most elements are: a long road or a far boundary doesn't shrink the building to a dot. The core is what has
// its middle within the middle 90 % of the elements' middles on each axis; then an element near it (within half its span,
// 3 m at least) and no more than half as big again joins, so a roof or a plinth stays in and a road along the site doesn't.
function coreBox(nodes, all) {
  const boxes = nodes.map(n => n.userData.box).filter(b => !b.isEmpty());
  if (boxes.length <= 20) return all.clone();
  const mids = boxes.map(b => b.getCenter(new THREE.Vector3()));
  const span = k => { const v = mids.map(m => m.getComponent(k)).sort((a, b) => a - b); return [v[Math.floor(v.length * 0.05)], v[Math.floor(v.length * 0.95)]]; };
  const s = [0, 1, 2].map(span), inside = (i, pad) => [0, 1, 2].every(k => mids[i].getComponent(k) >= s[k][0] - pad[k] && mids[i].getComponent(k) <= s[k][1] + pad[k]);
  const core = new THREE.Box3();
  boxes.forEach((b, i) => { if (inside(i, [0, 0, 0])) core.union(b); });
  if (core.isEmpty()) return all.clone();
  const size = core.getSize(new THREE.Vector3()), pad = s.map(([lo, hi]) => Math.max(3, (hi - lo) / 2));
  const near = core.clone();
  boxes.forEach((b, i) => {
    const bs = b.getSize(new THREE.Vector3());
    if (inside(i, pad) && [0, 1, 2].every(k => bs.getComponent(k) <= size.getComponent(k) * 1.5 + 1)) near.union(b);
  });
  return near;
}

// The start: a model is chosen, dropped on the page or named in the address (?file=); what goes wrong is said plainly.
const nameOf = label => { const last = label.split('/').pop(); try { return decodeURIComponent(last).replace(/\.glb$/i, ''); } catch { return last.replace(/\.glb$/i, ''); } };
// A model is coming: the ring, with no words.
const ring = new Ring($('ring'));
function loading() {
  $('drop').hidden = true;
  if (!ring.on) ring.start();
}
function failed(label, e) {
  ring.stop(); $('drop').hidden = false;
  $('pick').hidden = false; $('pick').textContent = 'Choose another model';
  $('droptitle').textContent = nameOf(label) + ' could not be opened';
  $('droptext').textContent = /^refused: /.test(e.message) ? e.message.slice(9)
    : /^404/.test(e.message) ? 'The file was not found. Check the link, or choose the model on this computer.'
    : /Failed to fetch|NetworkError|Load failed/i.test(e.message) ? 'The server could not be reached. Check your connection and try again.'
    : 'This file is not a BIMCraft model, or it is damaged. Save the model again from BIMCraft and open the new file.';
}
async function openFile(f) {
  if (!f) return;
  const t0 = performance.now();
  ring.stop(); loading();
  try { await load(await f.arrayBuffer(), f.name, t0); } catch (e) { failed(f.name, e); }
}
$('pick').onclick = () => $('file').click();
$('file').onchange = () => openFile($('file').files[0]);
view.addEventListener('dragover', e => { e.preventDefault(); $('drop').classList.add('over'); });
view.addEventListener('dragleave', e => { if (e.target === $('drop')) $('drop').classList.remove('over'); });
view.addEventListener('drop', e => { e.preventDefault(); $('drop').classList.remove('over'); openFile(e.dataTransfer.files[0]); });
const file = new URLSearchParams(location.search).get('file');
// The manual fallback to what follows the file by itself: a click on the dot fetches it again.
function offerRefresh(run) {
  const b = $('live');
  b.dataset.again = 'click to fetch the model again';
  b.onclick = async () => { b.disabled = true; try { await run(); } finally { setTimeout(() => { b.disabled = false; }, 800); } };
}
if (file) {
  const t0 = performance.now();
  loading();
  fetchWithProgress(file).then(b => load(b, file, t0)).catch(e => failed(file, e));
  const again = async () => { const t = performance.now(); await load(await (await fetch(file, { cache: 'no-store' })).arrayBuffer(), file, t, !!app.model); };
  show($('live'), 'live', 'Following the file');
  offerRefresh(() => updating($('live'), again));
  followFile(file, again, $('live'));
}
