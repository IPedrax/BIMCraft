// The viewer's light (`web/viewer/js/light.js`), run by `node --test web/tests/light.test.mjs`: each kind of face comes out as
// bright as the desktop's 3D view draws it (the core's `lighting`, which multiplies the colour as it is shown).
import test from 'node:test';
import assert from 'node:assert/strict';
import { LIGHT, SUN } from '../viewer/js/light.js';

const shown = l => (l <= 0.0031308 ? 12.92 * l : 1.055 * l ** (1 / 2.4) - 0.055);
// How bright three.js makes a white face, as shown: `up` is its normal's height, `sun` how square it is to the sun.
const face = (up, sun) => {
  const w = (up + 1) / 2, sky = (w + (1 - w) * LIGHT.ground) * LIGHT.sky;
  return shown((sky + LIGHT.sun * Math.max(sun, 0) + LIGHT.bounce * Math.max(-up, 0)) / Math.PI);
};
const near = (a, b) => assert.ok(Math.abs(a - b) < 0.005, `${a} is not ${b}`);

test('a wall out of the sun is as dark as the desktop draws it', () => near(face(0, 0), 0.55));
test('a roof out of the sun has the sky alone, as the desktop', () => near(face(1, 0), 0.65));
test('a sunlit roof comes out in its model colour', () => near(face(1, SUN[1] / Math.hypot(...SUN)), 1));
test('a ceiling has what the floor throws back, as the desktop', () => near(face(-1, 0), 0.9));
