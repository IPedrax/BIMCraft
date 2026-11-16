// A flat face's outline (`web/viewer/js/faces.js`), where measure labels go (`web/viewer/js/labels.js`) and when Measure snaps under the
// pointer (`web/viewer/js/hover.js`), with no browser.
// Run by `node --test web/tests/faces.test.mjs`.
import test from 'node:test';
import assert from 'node:assert/strict';
import { outline, onFace } from '../viewer/js/faces.js';
import { Hover } from '../viewer/js/hover.js';
import { layout, along, beside } from '../viewer/js/labels.js';
import { readFileSync } from 'node:fs';

const near = (a, b, tol = 1e-9) => assert.ok(Math.abs(a - b) <= tol, `${a} is not ${b}`);
const tri = (...p) => p.flat();
const lengths = o => o.segments.map(([a, b]) => Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2])).sort((x, y) => x - y);

test('a wall face of two triangles: four edges, the seam between them is not one, and its area', () => {
  const o = outline(tri([0, 0, 0], [3.4, 0, 0], [3.4, 1.7, 0], [0, 0, 0], [3.4, 1.7, 0], [0, 1.7, 0]));
  assert.deepEqual(lengths(o).map(v => +v.toFixed(6)), [1.7, 1.7, 3.4, 3.4]);
  near(o.area, 3.4 * 1.7);
  assert.deepEqual(o.centre.map(v => +v.toFixed(6)), [1.7, 0.85, 0]);
});

test('a straight edge cut into pieces by the triangles along it is one edge', () => {
  // 2 m by 1 m as a fan from a point on the bottom edge: that point and the top's middle stand in straight edges.
  const o = outline(tri([1, 0, 0], [2, 0, 0], [2, 1, 0], [1, 0, 0], [2, 1, 0], [1, 1, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0], [1, 0, 0], [0, 1, 0], [0, 0, 0]));
  assert.deepEqual(lengths(o), [1, 1, 2, 2]);
  near(o.area, 2);
});

test('a face is only what joins the triangle clicked: a separate patch on the same plane is not in it', () => {
  const o = outline(tri([0, 0, 0], [1, 0, 0], [0, 1, 0], [5, 5, 0], [6, 5, 0], [5, 6, 0]), 0);
  assert.equal(o.segments.length, 3);
  near(o.area, 0.5);
});

test('a frame round an opening has its outer edges and its inner ones, and the opening is not counted in its area', () => {
  // 3 by 3 with a 1 by 1 hole in the middle, as eight triangles.
  const q = (a, b, c, d) => tri(a, b, c, a, c, d);
  const P = (x, y) => [x, y, 0];
  const o = outline([
    ...q(P(0, 0), P(3, 0), P(2, 1), P(1, 1)), ...q(P(3, 0), P(3, 3), P(2, 2), P(2, 1)),
    ...q(P(3, 3), P(0, 3), P(1, 2), P(2, 2)), ...q(P(0, 3), P(0, 0), P(1, 1), P(1, 2))]);
  assert.deepEqual(lengths(o), [1, 1, 1, 1, 3, 3, 3, 3]);
  near(o.area, 8);
});

test('labels are laid most wanted first, each clear of the others; one that may go goes, one that must stays', () => {
  const box = { x0: 0, y0: 0, x1: 800, y1: 600 };
  const a = { x: 400, y: 300, w: 80, h: 24, prio: 0, tries: [[0, 0]], must: true };
  const b = { x: 405, y: 302, w: 80, h: 24, prio: 1, tries: [[0, 0], [0, 40]], must: true };
  const c = { x: 400, y: 300, w: 40, h: 20, prio: 2, tries: [[0, 0], [0, 40]], must: false };
  const [pa, pb, pc] = layout([c, b, a], box).reverse();
  assert.deepEqual(pa, { x: 400, y: 300 });
  assert.deepEqual(pb, { x: 405, y: 342 });
  assert.equal(pc, null);
});

test('a label that must show stays at its first place when nowhere is clear, and none is laid off the screen', () => {
  const box = { x0: 0, y0: 0, x1: 200, y1: 100 };
  const out = layout([{ x: 100, y: 50, w: 60, h: 20, prio: 0, tries: [[0, 0]], must: true },
    { x: 100, y: 50, w: 60, h: 20, prio: 1, tries: [[0, 0], [0, -200]], must: true },
    { x: 195, y: 50, w: 60, h: 20, prio: 2, tries: [[0, 0]], must: false }], box);
  assert.deepEqual(out, [{ x: 100, y: 50 }, { x: 100, y: 50 }, null]);
});

test('a label tries the line first, then off it either side; a tag goes off its leg on the side away from the length', () => {
  const t = along({ x: 0, y: 0 }, { x: 100, y: 0 }, 60, 20);
  assert.deepEqual(t[0], [0, 0]);
  near(Math.abs(t[1][1]), 16); near(t[1][0], 0);
  const s = beside({ x: 0, y: 0 }, { x: 100, y: 0 }, 40, 16, { x: 50, y: -30 });
  assert.ok(s[0][1] > 0, 'away from the length, below the leg');
  assert.deepEqual(s.slice(2).map(([x]) => x), [-25, -25, 25, 25], 'then a quarter of the way along, either side');
});

// Measure's hover: a face lit under the pointer must be only the face the pointer is on, and the
// lit face and the snap dot must follow the pointer as the view moves, never stay where they were while it orbits.
test('a thin plate lit from above lights its top only, not its underside 1 mm below it', () => {
  const n = [0, 0, 1], d = 0.1;
  assert.ok(onFace([0, 0, 0.1], [1, 0, 0.1], [0, 1, 0.1], n, d), 'the top, wound to face up');
  assert.ok(!onFace([0, 0, 0.099], [0, 1, 0.099], [1, 0, 0.099], n, d), 'the underside faces down: not the face pointed at');
  assert.ok(!onFace([0, 0, 0.1], [1, 0, 0.1], [0, 0, 0.0], n, d), 'a side is not on the plane');
  assert.ok(!onFace([0, 0, 0.1], [1, 0, 0.1], [2, 0, 0.1], n, d), 'a sliver has no way');
});

test('the dot and the lit face are hidden while a press moves the view, and found again where the pointer is when it lets go', () => {
  const h = new Hover();
  h.move(300, 130);
  assert.deepEqual(h.take(), { x: 300, y: 130 }, 'a move snaps at the pointer on the next frame');
  assert.equal(h.take(), null, 'once');
  h.move(310, 140); h.move(320, 150);
  assert.deepEqual(h.take(), { x: 320, y: 150 }, 'several moves in one frame: one snap, at the last');
  h.press();
  assert.equal(h.shown, false, 'pressed: nothing is shown under the pointer');
  h.view(); h.view();
  assert.equal(h.take(), null, 'the view orbits under a press: no snap, the old one is not kept either');
  h.release(330, 160);
  assert.equal(h.shown, true);
  assert.deepEqual(h.take(), { x: 330, y: 160 }, 'let go: snapped again where the pointer is');
});

test('a wheel or a flight moves the view under a still pointer: snapped again there; off the picture, nothing', () => {
  const h = new Hover();
  h.move(200, 100); h.take();
  h.view();
  assert.deepEqual(h.take(), { x: 200, y: 100 }, 'the view moved: what is under the pointer now');
  h.leave();
  assert.equal(h.shown, false);
  h.view();
  assert.equal(h.take(), null, 'the pointer is off the picture');
});

// The legs' colours, as measuring tools draw them (x #f12c2c, y #0bb80b, z #2c2cf1, solid,
// 2 px), the green darkened to #0a9f0a so each keeps 3:1 against its white halo over the darkest model face. measure.js
// imports three, so its table is read as text.
const luminance = hex => {
  const c = [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16) / 255).map(v => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
};
const contrast = (a, b) => { const [p, q] = [luminance(a), luminance(b)].sort((x, y) => y - x); return (p + 0.05) / (q + 0.05); };
const over = (alpha, fg, bg) => '#' + [1, 3, 5].map(i => Math.round(alpha * parseInt(fg.slice(i, i + 2), 16) + (1 - alpha) * parseInt(bg.slice(i, i + 2), 16)).toString(16).padStart(2, '0')).join('');

test('the legs are drawn solid and 2 px wide in saturated red, green and blue that keep 3:1 on white and on their halo', () => {
  const src = readFileSync(new URL('../viewer/js/measure.js', import.meta.url), 'utf8');
  const axes = src.match(/const AXES = \[(.*)\];/);
  assert.ok(axes, 'measure.js has its AXES table');
  const colours = [...axes[1].matchAll(/\['([xyz])', '(#[0-9a-f]{6})'/g)].map(m => [m[1], m[2]]);
  assert.deepEqual(colours, [['x', '#f12c2c'], ['y', '#0a9f0a'], ['z', '#2c2cf1']]);
  const width = src.match(/const LEG = (\d+(?:\.\d+)?);/);
  assert.ok(width && Number(width[1]) >= 2, 'a leg is at least 2 px, wide enough to draw its halo');
  assert.ok(/line\(t\.a, t\.b, LEG, AXES\[t\.axis\]\[1\]\)/.test(src), 'each leg is drawn solid in its own colour from the table');
  // The darkest model face seen under a measure: the halo, 85 % white, over it.
  const halo = over(0.85, '#ffffff', '#9d8479');
  for (const [axis, hex] of colours) {
    assert.ok(contrast(hex, '#ffffff') >= 3, `${axis} ${hex} on white: ${contrast(hex, '#ffffff').toFixed(2)}`);
    assert.ok(contrast(hex, halo) >= 3, `${axis} ${hex} on its halo ${halo}: ${contrast(hex, halo).toFixed(2)}`);
  }
  // Not the washed-out ones of before.
  for (const old of ['#e8483d', '#22a35a', '#3d7fe6']) assert.ok(!src.includes(old), `${old} is gone`);
});

// The snap indicator, under the pointer and at each placed point: the Autodesk viewer's snap orange (#ff7700, its Snapping
// extension; AutoCAD's AutoSnap marker is yellow), edged by a thin dark keyline rather than a thick white ring, so it reads on
// a light face by its keyline and on a dark face by its fill. The lines' white halo is thinner: 0.75 px a side.
test('the snap indicator is snap orange with a 1 px dark keyline, readable on light and dark faces; no white ring', () => {
  const src = readFileSync(new URL('../viewer/js/measure.js', import.meta.url), 'utf8');
  const constant = name => { const m = src.match(new RegExp(`const ${name} = '?([^';]+)'?;`)); assert.ok(m, `measure.js has ${name}`); return m[1]; };
  const snap = constant('SNAP'), key = constant('KEYLINE'), keyWidth = Number(constant('KEYLINE_W')), halo = Number(constant('HALO'));
  assert.equal(snap, '#ff7700');
  assert.equal(key, '#1a1a1a');
  assert.ok(keyWidth > 0 && keyWidth <= 1, `the keyline is thin: ${keyWidth} px`);
  assert.ok(halo > 0 && halo <= 1.5, `the lines' halo adds at most 1.5 px: ${halo}`);
  assert.ok(/const MARK = \{ ink: SNAP, halo: KEYLINE, haloW: 2 \* KEYLINE_W, ring: KEYLINE_W \};/.test(src),
    'every snap shape and end mark is drawn in the snap colour, edged by the keyline');
  assert.ok(!/0x1c5ca3/.test(src) && !/stroke="#fff"/.test(src), 'no blue dot, no white ring');
  for (const face of ['#ffffff', '#b4998d', '#9d8479', '#76a6bc']) assert.ok(contrast(key, face) >= 3, `keyline on a light face ${face}: ${contrast(key, face).toFixed(2)}`);
  for (const face of ['#333333', '#1e2a35', '#5a3a2e']) assert.ok(contrast(snap, face) >= 3, `fill on a dark face ${face}: ${contrast(snap, face).toFixed(2)}`);
  assert.ok(contrast(snap, key) >= 3, 'the fill stands off its own keyline');
});

// The legs' tags, as the Autodesk viewer's: a white pill with the number in its leg's colour, each at least 4.5:1 on the white
// (they were pale words on a black shadow, washed out on the model).
test('the legs are named on white pills in red, green and blue that read at 4.5:1 or better', () => {
  const css = readFileSync(new URL('../viewer/index.html', import.meta.url), 'utf8');
  const tag = css.match(/\.label\.mtag\{([^}]*)\}/);
  assert.ok(tag, 'index.html styles .label.mtag');
  assert.ok(/background:#fff(;|$)/.test(tag[1]), `a tag is a white pill: ${tag[1]}`);
  assert.ok(/border-radius:\d+px/.test(tag[1]) && !/text-shadow/.test(tag[1]), 'rounded, with no shadow behind its words');
  const colours = ['x', 'y', 'z'].map(a => { const m = css.match(new RegExp(`\\.mtag\\.${a}\\{color:(#[0-9a-f]{6})\\}`)); assert.ok(m, `.mtag.${a} has a colour`); return m[1]; });
  assert.deepEqual(colours, ['#ec1010', '#088a08', '#2c2cf1']);
  for (const c of colours) assert.ok(contrast(c, '#ffffff') >= 4.5, `${c} on white: ${contrast(c, '#ffffff').toFixed(2)}`);
});

// The face under the pointer is lit as the Autodesk viewer lights a snapped face (its Snapping extension: #00cc00 at 50 %),
// not light blue.
test('the face under the pointer is lit snap green at half strength', () => {
  const src = readFileSync(new URL('../viewer/js/measure.js', import.meta.url), 'utf8');
  assert.match(src, /const LIT = '#00cc00';/);
  assert.match(src, /const LIT_OPACITY = 0\.5;/);
  assert.match(src, /MeshBasicMaterial\(\{ color: LIT, transparent: true, opacity: LIT_OPACITY,/, 'the lit face is drawn in it');
  assert.ok(!/0x29b6f6/.test(src), 'the light blue is gone');
});
