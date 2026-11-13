// How a floating panel moves and sizes over the model (`web/viewer/js/panels.js`), run by `node --test web/tests/panels.test.mjs`.
import test from 'node:test';
import assert from 'node:assert/strict';
import { placed, resized, clearOf, overButton, MIN, GAP } from '../viewer/js/panels.js';

const area = { w: 1000, h: 600 }, box = { x: 300, y: 100, w: 300, h: 200 };

test('a panel set down inside the model stays where it was put', () => {
  assert.deepEqual(placed(box, area), box);
});

test('a panel dragged past an edge stops at it, whole', () => {
  assert.deepEqual(placed({ ...box, x: -500, y: -500 }, area), { ...box, x: GAP, y: GAP });
  assert.deepEqual(placed({ ...box, x: 5000, y: 5000 }, area), { ...box, x: 1000 - 300 - GAP, y: 600 - 200 - GAP });
});

test('a window smaller than a panel shrinks the panel, to its least size at most', () => {
  assert.deepEqual(placed({ x: 0, y: 0, w: 900, h: 900 }, { w: 400, h: 300 }), { x: GAP, y: GAP, w: 400 - 2 * GAP, h: 300 - 2 * GAP });
  assert.deepEqual(placed({ x: 50, y: 50, w: 900, h: 900 }, { w: 100, h: 100 }), { x: GAP, y: GAP, w: MIN.w, h: MIN.h });
});

test('each edge pulled moves that edge and no other', () => {
  assert.deepEqual(resized(box, 'e', 50, 9, area), { x: 300, y: 100, w: 350, h: 200 });
  assert.deepEqual(resized(box, 's', 9, 40, area), { x: 300, y: 100, w: 300, h: 240 });
  assert.deepEqual(resized(box, 'w', 50, 9, area), { x: 350, y: 100, w: 250, h: 200 });
  assert.deepEqual(resized(box, 'n', 9, 40, area), { x: 300, y: 140, w: 300, h: 160 });
});

test('a corner pulled moves its two edges', () => {
  assert.deepEqual(resized(box, 'se', 20, 30, area), { x: 300, y: 100, w: 320, h: 230 });
  assert.deepEqual(resized(box, 'nw', -20, -30, area), { x: 280, y: 70, w: 320, h: 230 });
});

test('a panel pulled too small stops at its least size and its far edge stays put', () => {
  assert.deepEqual(resized(box, 'w', 900, 0, area), { x: 600 - MIN.w, y: 100, w: MIN.w, h: 200 });
  assert.deepEqual(resized(box, 'n', 0, 900, area), { x: 300, y: 300 - MIN.h, w: 300, h: MIN.h });
  assert.deepEqual(resized(box, 'e', -900, 0, area), { x: 300, y: 100, w: MIN.w, h: 200 });
});

test('a panel pulled past the model stops at the edge and its far edge stays put', () => {
  assert.deepEqual(resized(box, 'w', -900, 0, area), { x: GAP, y: 100, w: 600 - GAP, h: 200 });
  assert.deepEqual(resized(box, 'se', 5000, 5000, area), { x: 300, y: 100, w: 1000 - GAP - 300, h: 600 - GAP - 100 });
});

test('the picture is framed between the panels, whichever side each now stands on', () => {
  assert.deepEqual(clearOf([], 1000), { x0: 0, x1: 1000 });
  assert.deepEqual(clearOf([{ left: 10, right: 260 }, { left: 660, right: 990 }], 1000), { x0: 268, x1: 652 });
  assert.deepEqual(clearOf([{ left: 700, right: 950 }, { left: 660, right: 990 }], 1000), { x0: 0, x1: 652 }, 'both moved to the right');
  assert.deepEqual(clearOf([{ left: 20, right: 300 }], 1000), { x0: 308, x1: 1000 }, 'moved, and still on the left');
});

test('a fly-out stands over its button, and stays whole at the sides of a narrow window', () => {
  assert.equal(overButton(500, 240, 1000), 500);
  assert.equal(overButton(40, 240, 400), 120 + GAP);
  assert.equal(overButton(380, 240, 400), 400 - 120 - GAP);
});
