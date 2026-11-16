// See Through's rules (`web/viewer/js/through.js`), run by `node --test web/tests/through.test.mjs`.
import test from 'node:test';
import assert from 'node:assert/strict';
import { LEVEL, MIN_LEVEL, MAX_LEVEL, CATEGORIES, settings, level, opacity, clears, behind, choose, pick, toggle } from '../viewer/js/through.js';

test('as the desktop opens it: off, 70 % clear, walls, floors, ceilings and roofs', () => {
  const s = settings();
  assert.equal(s.on, false);
  assert.equal(s.level, LEVEL);
  assert.equal(LEVEL, 70);
  assert.deepEqual([...s.categories].sort(), ['ceiling', 'floor', 'roof', 'wall']);
  assert.deepEqual(CATEGORIES, ['wall', 'floor', 'ceiling', 'roof']);
});

test('a level stays where a wall and its edges are still there, and what is not a number keeps the last one', () => {
  assert.equal(level(0), MIN_LEVEL);
  assert.equal(level(100), MAX_LEVEL);
  assert.equal(level('40'), 40);
  assert.equal(level(NaN, 55), 55);
  assert.equal(level(Infinity, 55), 55);
});

test('how clear a thing is goes on top of what it was, so glass stays clearer than a wall', () => {
  assert.ok(Math.abs(opacity(70) - 0.3) < 1e-9);
  assert.ok(Math.abs(opacity(50, 0.4) - 0.2) < 1e-9);
});

test('off, nothing is clear; on, what is of its categories or was picked', () => {
  const s = settings();
  assert.equal(clears(s, 'wall', 'a'), false);
  s.on = true;
  assert.equal(clears(s, 'wall', 'a'), true);
  assert.equal(clears(s, 'pipe', 'b'), false);
  s.picked.add('b');
  assert.equal(clears(s, 'pipe', 'b'), true);
  s.categories.delete('wall');
  assert.equal(clears(s, 'wall', 'a'), false);
});

test('off keeps the level, the categories and what was picked for when it is turned on again', () => {
  const s = settings();
  s.on = true; s.level = 40; s.picked.add('b'); s.categories.delete('roof');
  s.on = false;
  assert.equal(clears(s, 'pipe', 'b'), false);
  s.on = true;
  assert.equal(s.level, 40);
  assert.equal(clears(s, 'pipe', 'b'), true);
  assert.equal(clears(s, 'roof', 'c'), false);
});

test('Tab steps back through what stands under the pointer, and from the last to the first', () => {
  const pipe = {}, wall = {}, slab = {};
  assert.equal(behind([wall, pipe, wall, slab], null), wall);
  assert.equal(behind([wall, pipe, slab], wall), pipe);
  assert.equal(behind([wall, pipe, slab], slab), wall);
  assert.equal(behind([wall, pipe], slab), wall);
  assert.equal(behind([], wall), null);
});

test('from off, a category in the menu makes that one clear and no other', () => {
  const s = settings();
  choose(s, 'floor');
  assert.equal(s.on, true);
  assert.deepEqual([...s.categories], ['floor']);
  assert.equal(clears(s, 'wall', 'a'), false);
});

test('while on, a category in the menu adds or takes away, and with nothing left it is off', () => {
  const s = settings();
  choose(s, 'floor'); choose(s, 'roof');
  assert.deepEqual([...s.categories].sort(), ['floor', 'roof']);
  choose(s, 'floor'); assert.equal(s.on, true);
  choose(s, 'roof'); assert.equal(s.on, false);
});

test('from off, the selection makes that element clear and no category', () => {
  const s = settings();
  pick(s, 'a');
  assert.equal(s.on, true);
  assert.equal(clears(s, 'pipe', 'a'), true);
  assert.equal(clears(s, 'wall', 'b'), false);
  pick(s, 'a');
  assert.equal(s.on, false);
});

test('the key brings back what was clear, or the four categories if nothing was', () => {
  const s = settings();
  choose(s, 'roof');
  toggle(s); assert.equal(s.on, false);
  toggle(s); assert.deepEqual([...s.categories], ['roof']);
  choose(s, 'roof'); assert.equal(s.on, false);
  toggle(s); assert.deepEqual([...s.categories].sort(), ['ceiling', 'floor', 'roof', 'wall']);
});
