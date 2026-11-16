// What a section shows on screen (`web/viewer/js/cut.js`), run by `node --test web/tests/cut.test.mjs`.
import test from 'node:test';
import assert from 'node:assert/strict';
import { planeInBox, boxFace, clip, middle, handleSize, closes } from '../viewer/js/cut.js';

const near = (a, b, tol = 1e-9) => assert.ok(Math.abs(a - b) <= tol, `${a} is not ${b}`);
// The area of a flat outline in x and y.
const area = p => Math.abs(p.reduce((s, a, i) => { const b = p[(i + 1) % p.length]; return s + a[0] * b[1] - b[0] * a[1]; }, 0)) / 2;

test('a plane square to an axis crosses a box in a rectangle, its corners in order round it', () => {
  const poly = planeInBox([0, 0, 0], [2, 3, 4], [0, 0, 1], [1, 1, 2]);
  assert.equal(poly.length, 4);
  near(area(poly), 6);
  poly.forEach(p => near(p[2], 2));
});

test('a plane aslant a cube through its middle crosses it in six corners', () => {
  assert.equal(planeInBox([0, 0, 0], [1, 1, 1], [1, 1, 1], [0.5, 0.5, 0.5]).length, 6);
});

test('a plane that misses the box has no outline', () => {
  assert.deepEqual(planeInBox([0, 0, 0], [1, 1, 1], [1, 0, 0], [5, 0, 0]), []);
});

test('a face of a box is its four corners on that side, in order round it', () => {
  const low = boxFace([0, 0, 0], [2, 3, 4], 'y', false), high = boxFace([0, 0, 0], [2, 3, 4], 'x', true);
  assert.equal(low.length, 4); low.forEach(p => near(p[1], 0));
  high.forEach(p => near(p[0], 2));
  near(area(boxFace([0, 0, 0], [2, 3, 4], 'z', true)), 6);
});

test('clipping keeps what is on the plane\'s positive side', () => {
  const square = [[0, 0, 0], [2, 0, 0], [2, 2, 0], [0, 2, 0]];
  const half = clip(square, [[-1, 0, 0, 1]]); // keeps x <= 1
  near(area(half), 2);
  assert.deepEqual(clip(square, [[-1, 0, 0, -5]]), [], 'all of it is past the plane');
  assert.equal(clip(square, [[1, 0, 0, 1]]).length, 4, 'all of it is kept');
});

test('the middle of a flat outline is its middle by area, not by its corners', () => {
  const m = middle([[0, 0], [2, 0], [2, 2], [0, 2]]);
  near(m[0], 1); near(m[1], 1);
  const crowded = middle([[0, 0], [1, 0], [2, 0], [2, 2], [0, 2]]);
  near(crowded[0], 1); near(crowded[1], 1);
  const t = middle([[0, 2], [0, 0], [3, 0]]);
  near(t[0], 1); near(t[1], 2 / 3);
});

test('an outline with no area has no middle: a plane seen edge on', () => {
  assert.equal(middle([[0, 0], [1, 1], [2, 2]]), null);
  assert.equal(middle([]), null);
});

test('a handle in a parallel view is as big as the view is wide, wherever it stands', () => {
  const size = depth => handleSize({ parallel: true, dist: 10, depth });
  near(size(50), size(0.01));
  near(size(-30), size(10));
  assert.ok(handleSize({ parallel: true, dist: 20, depth: 3 }) > size(3), 'a wider view, a bigger handle in the model');
});

test('a handle in a perspective view grows with how far ahead it stands, not how far aside', () => {
  near(handleSize({ parallel: false, dist: 1, depth: 10 }), handleSize({ parallel: false, dist: 500, depth: 10 }));
  near(handleSize({ parallel: false, dist: 1, depth: 20 }), 2 * handleSize({ parallel: false, dist: 1, depth: 10 }));
});

test('a handle always has a size', () => {
  assert.ok(handleSize({ parallel: false, dist: 1, depth: 0 }) > 0);
  assert.ok(handleSize({ parallel: true, dist: 0, depth: 0 }) > 0);
});

// A cube's twelve triangles, wound outward, as flat positions.
const CUBE = [[0, 1, 3, 0, 3, 2], [4, 6, 7, 4, 7, 5], [0, 4, 5, 0, 5, 1], [2, 3, 7, 2, 7, 6], [0, 2, 6, 0, 6, 4], [1, 5, 7, 1, 7, 3]].flat()
  .flatMap(i => [i & 1, i >> 1 & 1, i >> 2 & 1]);

test('a solid closes, by flat positions or by an index, and one with a face missing does not', () => {
  assert.equal(closes(CUBE), true);
  assert.equal(closes(CUBE.slice(0, -18)), false, 'one side of the cube left off');
  const corners = [0, 1, 2, 3, 4, 5, 6, 7].flatMap(i => [i & 1, i >> 1 & 1, i >> 2 & 1]);
  const index = [0, 1, 3, 0, 3, 2, 4, 6, 7, 4, 7, 5, 0, 4, 5, 0, 5, 1, 2, 3, 7, 2, 7, 6, 0, 2, 6, 0, 6, 4, 1, 5, 7, 1, 7, 3];
  assert.equal(closes(corners, index), true);
});

test('a solid inside out still closes, and nothing has no faces to close', () => {
  const inside = []; for (let i = 0; i < CUBE.length; i += 9) inside.push(...CUBE.slice(i + 6, i + 9), ...CUBE.slice(i + 3, i + 6), ...CUBE.slice(i, i + 3));
  assert.equal(closes(inside), true);
  assert.equal(closes([]), false);
});
