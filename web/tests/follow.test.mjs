// A page opened with ?file= follows the file: it asks the same address whether it changed, and fetches it again when it did.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { followFile } from '../viewer/js/follow.js';

function page(marks) {
  const asked = [];
  globalThis.document = { hidden: false };
  globalThis.fetch = async (url, how) => {
    asked.push([url, how.method]);
    const mark = marks.shift();
    if (mark instanceof Error) throw mark;
    return { ok: true, headers: { get: k => (k === 'etag' ? mark : null) } };
  };
  return asked;
}
// The page's timer never stops; here it stops with each test, so the run ends.
const every = globalThis.setInterval, timers = [];
globalThis.setInterval = (f, ms) => { const t = every(f, ms); timers.push(t); return t; };
// It waits for what the test expects to have happened, 2 s at most: a busy machine runs a 5 ms timer late.
const settle = async (done = () => true) => {
  const end = Date.now() + 2000;
  await new Promise(r => setTimeout(r, 25));
  while (!done() && Date.now() < end) await new Promise(r => setTimeout(r, 5));
  timers.splice(0).forEach(clearInterval);
};

test('a file that changed is fetched again, one that did not is not, and a failed look is passed over', async () => {
  const asked = page(['a', 'a', new Error('no connection'), 'b', 'b']);
  let reloads = 0;
  const chip = { dataset: {} };
  const reload = async () => { reloads++; };
  followFile('house.glb', reload, chip, 5);
  await settle(() => asked.length >= 5);
  assert.equal(reloads, 1);
  assert.equal(chip.dataset.state, 'live');
  assert.ok(asked.every(([url, method]) => url === 'house.glb' && method === 'HEAD'), 'it asks only the address it was given');
});

test('a hidden page asks nothing', async () => {
  const asked = page(['a']);
  document.hidden = true;
  followFile('house.glb', async () => {}, { dataset: {} }, 5);
  await settle();
  assert.equal(asked.length, 0);
});
