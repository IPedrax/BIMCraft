// What a section cuts is closed with a flat face on the cut, as a CAD program fills a section. The faces of everything drawn are
// counted through the stencil, one plane at a time: past a cut, a point inside a solid has seen one more back face than front
// faces. Where the count is not nought a sheet on the plane is drawn. Faces that touch (a wall on its slab, a wall and its
// finish) can't hide such a cap, as they hid a fill made of back faces.
import * as THREE from 'three';
import { closes } from './cut.js';

const counting = (side, op) => new THREE.MeshBasicMaterial({
  side, colorWrite: false, depthWrite: false, depthTest: false, stencilWrite: true, stencilFunc: THREE.AlwaysStencilFunc,
  stencilFail: op, stencilZFail: op, stencilZPass: op,
});

// Whether a mesh's faces can close: the face areas of a closed solid, as vectors, add up to nothing. An open one would leave its
// count standing over everything behind it, so it stays out, and what a cut shows of it is its back faces.
const known = new WeakMap();
const closed = g => {
  if (!known.has(g)) known.set(g, closes(g.attributes.position.array, g.index?.array));
  return known.get(g);
};

// Where a mesh stands in the world; a model's meshes do not move once loaded.
const boxes = new WeakMap();
const boxOf = m => {
  if (!boxes.has(m)) {
    if (!m.geometry.boundingBox) m.geometry.computeBoundingBox();
    boxes.set(m, m.geometry.boundingBox.clone().applyMatrix4(m.matrixWorld));
  }
  return boxes.get(m);
};

export class Caps {
  constructor(app, colour) {
    this.app = app;
    this.back = counting(THREE.BackSide, THREE.IncrementWrapStencilOp);
    this.front = counting(THREE.FrontSide, THREE.DecrementWrapStencilOp);
    const stencil = { side: THREE.DoubleSide, stencilWrite: true, stencilFunc: THREE.NotEqualStencilFunc, stencilRef: 0,
      stencilFail: THREE.KeepStencilOp, stencilZFail: THREE.KeepStencilOp, stencilZPass: THREE.KeepStencilOp };
    this.fill = new THREE.MeshBasicMaterial({ color: colour, toneMapped: false, ...stencil });
    this.normal = new THREE.MeshNormalMaterial(stencil);
    this.sheet = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), this.fill);
    this.sheet.frustumCulled = false;
    this.stage = new THREE.Scene(); this.stage.add(this.sheet);
  }
  // Over what `cam` has just drawn to the target in use: `normals` when that target holds the view's normals for the ink.
  draw(cam, normals = false) {
    const a = this.app, R = a.renderer, planes = R.clippingPlanes;
    if (!planes?.length || !a.model) return;
    const { scene } = a, bg = scene.background, ground = a.look?.ground, groundWas = ground?.visible;
    // Only what was drawn and closes can be counted; a piece no plane crosses counts nothing, so it costs nothing.
    const shown = a.meshes.filter(m => m.visible), solid = shown.filter(m => !m.material.transparent && closed(m.geometry));
    shown.forEach(m => { m.visible = false; });
    if (ground) ground.visible = false;
    scene.background = null;
    const shadows = R.shadowMap.enabled; R.shadowMap.enabled = false; const auto = R.shadowMap.autoUpdate; R.shadowMap.autoUpdate = false;
    const box = a.box, c = box.getCenter(new THREE.Vector3()), s = box.getSize(new THREE.Vector3()).length() * 2.2 + 2;
    this.sheet.material = normals ? this.normal : this.fill;
    this.sheet.scale.set(s, s, 1);
    planes.forEach((plane, i) => {
      const crossed = solid.filter(m => plane.intersectsBox(boxOf(m)));
      if (!crossed.length) return;
      crossed.forEach(m => { m.visible = true; });
      R.clearStencil();
      R.clippingPlanes = [plane];
      scene.overrideMaterial = this.back; R.render(scene, cam);
      scene.overrideMaterial = this.front; R.render(scene, cam);
      scene.overrideMaterial = null;
      crossed.forEach(m => { m.visible = false; });
      R.clippingPlanes = planes.filter((_, k) => k !== i);
      plane.projectPoint(c, this.sheet.position);
      this.sheet.quaternion.setFromUnitVectors(new THREE.Vector3(0, 0, 1), plane.normal);
      this.sheet.updateMatrixWorld(true);
      R.render(this.stage, cam);
    });
    R.clippingPlanes = planes;
    scene.background = bg; shown.forEach(m => { m.visible = true; });
    if (ground) ground.visible = groundWas;
    R.shadowMap.enabled = shadows; R.shadowMap.autoUpdate = auto;
  }
}
