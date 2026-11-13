// How the walker's body moves (`web/viewer/js/body.js`), run by `node --test web/tests/body.test.mjs`.
import test from 'node:test';
import assert from 'node:assert/strict';
import { fall, rise, liftTo, roll, glide, MAX_FALL, ROLL_MOST } from '../viewer/js/body.js';

const near = (a, b, tol = 1e-9) => assert.ok(Math.abs(a - b) <= tol, `${a} is not ${b}`);
const DT = 1 / 60;
// Fall from `y` to `floor` a frame at a time; the seconds it took and the fastest it went.
function drop(y, floor) {
  let vy = 0, t = 0, fastest = 0;
  while (y > floor && t < 100) { ({ y, vy } = fall(y, floor, vy, DT)); t += DT; fastest = Math.max(fastest, -vy); }
  return { y, t, fastest };
}

test('with no floor under it the body holds its height, however long', () => {
  let s = { y: 80, vy: 0 };
  for (let i = 0; i < 600; i++) s = fall(s.y, null, s.vy, DT);
  assert.deepEqual(s, { y: 80, vy: 0 });
});

test('off the edge of a floor a fall stops where it is', () => {
  assert.deepEqual(fall(20, null, -7, DT), { y: 20, vy: 0 });
});

test('a fall builds up and never passes 10 m/s', () => {
  assert.ok(drop(150, 0).fastest <= MAX_FALL);
  near(MAX_FALL, 10);
});

test('a long fall takes about as long as Autodesk\'s: 50 m in 5.5 s, not the 3.2 s of plain gravity', () => {
  near(drop(50, 0).t, 5.5, 0.1);
});

test('from a room\'s height it comes down in under a second, and stops on the floor', () => {
  const d = drop(2, 0);
  near(d.y, 0);
  near(d.t, 0.64, 0.05);
});

test('a floor at the feet or just over them is stood on at once', () => {
  assert.deepEqual(fall(1, 1.3, 0, DT), { y: 1.3, vy: 0 });
  assert.deepEqual(fall(1, 1, -4, DT), { y: 1, vy: 0 });
});

test('an eye lower than eye height over a floor stands up on that floor, not through it', () => {
  assert.equal(liftTo(1, 0, 1.6), 0);
  assert.equal(liftTo(0.05, 0, 1.6), 0);
  assert.equal(liftTo(1.6, 0, 1.6), null);
  assert.equal(liftTo(3, 0, 1.6), null);
  assert.equal(liftTo(1, null, 1.6), null);
});

test('standing up is gentle and stops at the height', () => {
  let y = 0, frames = 0;
  while (y < 1 && frames < 500) { const next = rise(y, 1, DT); assert.ok(next - y < 0.05); y = next; frames++; }
  assert.equal(y, 1);
  assert.ok(frames > 20);
});

test('a notch of the wheel is 2.1 m, forward when rolled away, and it does not pile up past what a flick gives', () => {
  near(roll(0, { deltaY: -100, deltaMode: 0 }), 2.1);
  near(roll(0, { deltaY: 100, deltaMode: 0 }), -2.1);
  near(roll(0, { deltaY: -3, deltaMode: 1 }), 2.1, 0.03);
  let left = 0;
  for (let i = 0; i < 50; i++) left = roll(left, { deltaY: -100, deltaMode: 0 });
  near(left, ROLL_MOST);
});

test('a small wheel event still moves, as a touch pad and a Mac wheel send them', () => {
  assert.ok(roll(0, { deltaY: -4, deltaMode: 0 }) >= 0.5);
});

test('a roll is covered in about half a second, and all of it', () => {
  let left = 2.1, gone = 0, t = 0;
  while (t < 0.5) { const g = glide(left, DT); gone += g.step; left = g.left; t += DT; }
  assert.ok(gone > 1.8 && gone < 2.1, `${gone}`);
  for (let i = 0; i < 300; i++) { const g = glide(left, DT); gone += g.step; left = g.left; }
  assert.equal(left, 0);
  near(gone, 2.1);
});
