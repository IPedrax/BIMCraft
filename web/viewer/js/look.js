// Look: edge lines and shadows, each turned on or off in the Look menu and remembered in this browser.
//
// Edges are found on the picture, not in the triangles, so they cost the same for a house as for a tower: the model is
// drawn a second time with its normals into a target of the picture's size, and a pass over it inks a line where the
// facing turns sharply or the depth breaks. A curved surface's facets turn little, so they stay smooth, as the desktop's
// 3D view draws them. The sun casts shadows over the building and onto a ground under it; they don't move with the
// camera, so the shadow map is drawn again only when what is shown changes.
import * as THREE from 'three';

const EDGE_VERT = `varying vec2 vUv; void main() { vUv = uv; gl_Position = vec4(position.xy, 0.0, 1.0); }`;
// A crease where two neighbours' normals part by more than about 35°; a break where the depth, as 1/distance (straight
// across a flat face on the screen; in a parallel view the depth itself is, and a break is measured against what is looked
// at, a parallel view's eye being nowhere in particular), bends between a pixel's two sides; an outline where the model meets
// the background.
// A speck is no edge: a hairline gap between two faces shows a pixel of what is behind, and it comes and goes as the camera
// moves. So a pixel whose two sides lie on one face (by their normals and the slope of their depths) is left uninked, and
// a side that is such a speck, with the pixel past it on the centre's face, is read as that face.
const EDGE_FRAG = `
uniform sampler2D tNormal; uniform sampler2D tDepth; uniform vec2 px; uniform float near; uniform float far; uniform vec3 ink; uniform float alpha; uniform float parallel; uniform float focus;
varying vec2 vUv;
float inv(float d) {
  if (parallel > 0.5) return d * (far - near);
  float z = d * 2.0 - 1.0; return (far + near - z * (far - near)) / (2.0 * near * far);
}
float at(vec2 o) { return texture2D(tDepth, vUv + o * px).r; }
vec3 nrm(vec2 o) { return texture2D(tNormal, vUv + o * px).rgb * 2.0 - 1.0; }
void main() {
  float d0 = at(vec2(0.0));
  if (d0 >= 1.0) discard;
  vec3 n0 = nrm(vec2(0.0)); float w0 = inv(d0), r0 = parallel > 0.5 ? focus : w0;
  float e = 0.0;
  vec2 dirs[2]; dirs[0] = vec2(1.0, 0.0); dirs[1] = vec2(0.0, 1.0);
  for (int i = 0; i < 2; i++) {
    vec2 o = dirs[i];
    float a = at(o), b = at(-o), a2 = at(2.0 * o), b2 = at(-2.0 * o);
    float wa = inv(a), wb = inv(b), wa2 = inv(a2), wb2 = inv(b2);
    vec3 na = nrm(o), nb = nrm(-o);
    bool ma = a < 1.0, mb = b < 1.0;
    if (ma && mb && a2 < 1.0 && b2 < 1.0 && dot(na, nb) > 0.95 && abs((wa - wb) - (wa2 - wa) - (wb - wb2)) < 0.004 * r0) continue;
    if (mb && a2 < 1.0 && dot(nrm(2.0 * o), n0) > 0.95 && abs(wa2 - (3.0 * w0 - 2.0 * wb)) < 0.006 * r0) { wa = 2.0 * w0 - wb; na = n0; ma = true; }
    if (ma && b2 < 1.0 && dot(nrm(-2.0 * o), n0) > 0.95 && abs(wb2 - (3.0 * w0 - 2.0 * wa)) < 0.006 * r0) { wb = 2.0 * w0 - wa; nb = n0; mb = true; }
    if (!ma || !mb) { e = 1.0; continue; }
    if (abs(wa + wb - 2.0 * w0) > 0.012 * r0) e = 1.0;
    if (dot(n0, na) < 0.82 || dot(n0, nb) < 0.82) e = max(e, 0.85);
  }
  if (e == 0.0) discard;
  gl_FragColor = vec4(ink, e * alpha);
}`;

function remembered(key, otherwise) {
  try { const v = localStorage.getItem('bimcraft-viewer-' + key); return v === null ? otherwise : v === '1'; } catch { return otherwise; }
}
function remember(key, on) {
  try { localStorage.setItem('bimcraft-viewer-' + key, on ? '1' : '0'); } catch { /* a private window keeps nothing */ }
}

export class Look {
  constructor(app, sun) {
    this.app = app; this.sun = sun;
    this.edges = remembered('edges', true); this.shadows = remembered('shadows', true);
    const R = app.renderer;
    const depth = new THREE.DepthTexture(1, 1, THREE.UnsignedInt248Type);
    depth.format = THREE.DepthStencilFormat;
    this.target = new THREE.WebGLRenderTarget(1, 1, { stencilBuffer: true, depthTexture: depth, type: THREE.UnsignedByteType });
    this.normals = new THREE.MeshNormalMaterial({ side: THREE.DoubleSide });
    this.quad = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), new THREE.ShaderMaterial({
      vertexShader: EDGE_VERT, fragmentShader: EDGE_FRAG, transparent: true, depthTest: false, depthWrite: false,
      uniforms: { tNormal: { value: this.target.texture }, tDepth: { value: this.target.depthTexture }, px: { value: new THREE.Vector2() },
        near: { value: 0.1 }, far: { value: 100 }, ink: { value: new THREE.Color(0x1d2530) }, alpha: { value: 0.6 }, parallel: { value: 0 }, focus: { value: 10 } },
    }));
    this.quad.frustumCulled = false;
    this.flat = new THREE.Scene(); this.flat.add(this.quad);
    this.flatCam = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
    // The sun's shadows: drawn when asked (`stale`), fitted to the building.
    R.shadowMap.type = THREE.PCFSoftShadowMap; R.shadowMap.autoUpdate = false;
    sun.shadow.mapSize.set(4096, 4096); sun.shadow.bias = -0.0004; sun.shadow.normalBias = 0.03;
    app.scene.add(sun.target);
    this.ground = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), new THREE.ShadowMaterial({ opacity: 0.22 }));
    this.ground.rotation.x = -Math.PI / 2; this.ground.receiveShadow = true; this.ground.visible = false;
    app.scene.add(this.ground);
    for (const ev of ['visibility', 'section', 'load']) app.on(ev, () => this.stale());
    app.on('load', () => this.fit());
    const pop = document.getElementById('lookpop'), btn = document.getElementById('b-look');
    btn.onclick = () => { pop.hidden = !pop.hidden; document.getElementById('secpop').hidden = true; };
    pop.addEventListener('click', e => {
      const b = e.target.closest('button'); if (!b) return;
      if (b.dataset.look === 'edges') this.setEdges(!this.edges);
      if (b.dataset.look === 'shadows') this.setShadows(!this.shadows);
    });
    this.setShadows(this.shadows, true); this.buttons();
  }
  buttons() {
    document.querySelectorAll('#lookpop button').forEach(b => b.setAttribute('aria-pressed', String(b.dataset.look === 'edges' ? this.edges : this.shadows)));
  }
  setEdges(on) { this.edges = on; remember('edges', on); this.buttons(); this.app.redraw(); }
  setShadows(on, first) {
    const app = this.app, R = app.renderer;
    this.shadows = on; if (!first) remember('shadows', on);
    R.shadowMap.enabled = on; this.sun.castShadow = on; this.ground.visible = on && !!app.model;
    // The materials' shaders differ with shadows and without: they are made again.
    app.remakeMaterials?.();
    this.buttons(); this.stale();
  }
  // The sun over the middle of the building, its shadow's box round it, and the ground just under the lowest point of the
  // model, so a site that has its own ground stands on this one instead of floating over it.
  fit() {
    const app = this.app, box = app.box, c = box.getCenter(new THREE.Vector3()), r = box.getSize(new THREE.Vector3()).length() / 2 + 1;
    const dir = new THREE.Vector3(0.55, 1.0, 0.75).normalize();
    this.sun.position.copy(c).addScaledVector(dir, r * 2); this.sun.target.position.copy(c); this.sun.target.updateMatrixWorld();
    const cam = this.sun.shadow.camera;
    Object.assign(cam, { left: -r, right: r, top: r, bottom: -r, near: 0.1, far: r * 4 }); cam.updateProjectionMatrix();
    this.ground.scale.set(r * 6, r * 6, 1); this.ground.position.set(c.x, Math.min(box.min.y, app.all.min.y) - 0.002, c.z); this.ground.visible = this.shadows;
    for (const m of app.meshes) { m.castShadow = !m.material.transparent; m.receiveShadow = true; }
    this.stale();
  }
  stale() { if (this.shadows) this.app.renderer.shadowMap.needsUpdate = true; this.app.redraw(); }
  // After the model is drawn: its edges inked over it.
  draw() {
    if (!this.edges || !this.app.model) return;
    const app = this.app, R = app.renderer, size = R.getDrawingBufferSize(new THREE.Vector2());
    if (this.target.width !== size.x || this.target.height !== size.y) this.target.setSize(size.x, size.y);
    // Glass and the ground stay out of the normals, so what is behind glass keeps its edges.
    this.ink(size, 0.6, app.meshes.filter(m => m.visible && m.material.transparent));
    // What See Through made clear keeps its own edges, fainter, drawn by themselves so they break no one else's.
    if (app.meshes.some(m => m.visible && m.material.userData.clear)) this.ink(size, 0.3, app.meshes.filter(m => m.visible && !m.material.userData.clear));
  }
  // Edges of what is drawn with the meshes of `hidden` left out.
  // What marks the model (Measure's lit face, `userData.inkless`) stays out too: drawn into the normals on the face it lies on,
  // it fought that face for its pixels and was inked in streaks that moved with the view.
  ink(size, alpha, hidden) {
    const app = this.app, R = app.renderer;
    hidden = [...hidden, ...app.scene.children.filter(o => o.visible && o.userData.inkless)];
    hidden.forEach(m => { m.visible = false; });
    const ground = this.ground.visible, bg = app.scene.background;
    const side = app.cut.value > 0.5 ? THREE.DoubleSide : THREE.FrontSide;
    if (this.normals.side !== side) { this.normals.side = side; this.normals.needsUpdate = true; }
    this.ground.visible = false; app.scene.background = null; app.scene.overrideMaterial = this.normals;
    const seen = app.camera;
    R.setRenderTarget(this.target); R.setClearColor(0x000000, 0); R.clear(); R.render(app.scene, seen);
    app.scene.overrideMaterial = null; app.caps?.draw(seen, true); R.setRenderTarget(null);
    app.scene.background = bg; this.ground.visible = ground;
    hidden.forEach(m => { m.visible = true; });
    const u = this.quad.material.uniforms;
    u.alpha.value = alpha; u.px.value.set(1 / size.x, 1 / size.y); u.near.value = seen.near; u.far.value = seen.far;
    u.parallel.value = app.parallel ? 1 : 0; u.focus.value = app.nav.dist;
    const planes = R.clippingPlanes; R.clippingPlanes = [];
    R.render(this.flat, this.flatCam);
    R.clippingPlanes = planes;
  }
}
