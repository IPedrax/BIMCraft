// The loader's ring: `node --test web/tests/ring.test.mjs`.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { DELAY, Ring, SPANS, overall } from '../viewer/js/ring.js';

const page = readFileSync(new URL('../viewer/index.html', import.meta.url), 'utf8');

test('the steps share the ring end to end', () => {
  const order = ['receiving', 'reading', 'placing'].map(s => SPANS[s]);
  assert.equal(order[0][0], 0);
  assert.equal(order.at(-1)[1], 1);
  for (let k = 1; k < order.length; k++) assert.equal(order[k][0], order[k - 1][1], 'no gap and no overlap');
  assert.equal(overall('receiving', 0), 0);
  assert.equal(overall('receiving', 0.5), 0.425);
  assert.equal(overall('placing', 2), 1, 'past the end is the end');
  assert.equal(overall('placing', NaN), null);
  assert.equal(overall('nonsense', 0.5), null);
});

// A ring of the test's own: an element's few parts, and timers it fires itself.
function ring() {
  const arc = { style: {} }, pct = { textContent: '' };
  const classes = new Set();
  const el = { hidden: true, querySelector: s => (s === '.arc' ? arc : pct), classList: { toggle: (c, on) => (on ? classes.add(c) : classes.delete(c)) } };
  const due = [];
  const timers = { set: (f, ms) => { due.push({ f, ms }); return due.length; }, clear: n => { if (due[n - 1]) due[n - 1].f = () => {}; } };
  return { r: new Ring(el, 26, timers), el, arc, pct, classes, fire: () => due.splice(0).forEach(t => t.f()), due };
}

test('the ring shows only once a load has taken a while, spins until counted, and never goes back', () => {
  const { r, el, pct, classes, fire, due } = ring();
  r.start();
  assert.equal(el.hidden, true, 'nothing at once');
  assert.equal(due[0].ms, DELAY);
  assert.ok(classes.has('spin'), 'nothing counted yet: it spins, with no number');
  assert.equal(pct.textContent, '');
  fire();
  assert.equal(el.hidden, false);
  r.at('receiving', 0.5);
  assert.equal(pct.textContent, '42%');
  assert.ok(!classes.has('spin'));
  r.at('reading', 0);
  assert.equal(pct.textContent, '85%', 'the bytes done, the reading starts');
  r.at('receiving', 0.1);
  assert.equal(pct.textContent, '85%', 'never back');
  r.at('placing', 1);
  assert.equal(pct.textContent, '99%', 'a hundred only as the model shows');
  r.stop();
  assert.equal(el.hidden, true);
});

test('a load done before the ring is due never shows it', () => {
  const { r, el, fire } = ring();
  r.start();
  r.at('placing', 1);
  r.stop();
  fire();
  assert.equal(el.hidden, true);
});

test('the page has the ring and the loader says no words', () => {
  const at = page.indexOf('id="ring"');
  assert.ok(at > 0, 'the ring is on the page');
  const el = page.slice(page.lastIndexOf('<', at), page.indexOf('</div>', at));
  assert.match(el, /class="arc"/);
  assert.match(el, /class="pct"/);
  assert.doesNotMatch(el.replace(/<[^>]*>/g, ''), /[A-Za-z]/, 'no words in the ring');
  const main = readFileSync(new URL('../viewer/js/main.js', import.meta.url), 'utf8');
  for (const words of ['Getting it ready to show', 'Downloading…', 'Reading the model…', 'Reading the file…']) {
    assert.ok(!main.includes(words), `no "${words}"`);
  }
});
