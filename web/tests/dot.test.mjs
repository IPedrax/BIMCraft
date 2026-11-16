// The status dot and the tool bar it took Refresh from: `node --test web/tests/dot.test.mjs`.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { show, updating } from '../viewer/js/follow.js';

const page = readFileSync(new URL('../viewer/index.html', import.meta.url), 'utf8');
const bar = page.slice(page.indexOf('<nav id="bar">'), page.indexOf('</nav>'));
const pop = page.slice(page.indexOf('<div id="vispop"'), page.indexOf('</div>', page.indexOf('<div id="vispop"')));

test('the dot holds its words for the hover and shows none', () => {
  const chip = { dataset: {}, hidden: true, textContent: '' };
  show(chip, 'wait', 'Updating');
  assert.equal(chip.hidden, false);
  assert.equal(chip.dataset.state, 'wait');
  assert.equal(chip.dataset.tip, 'Updating');
  assert.equal(chip.ariaLabel, 'Updating');
  assert.equal(chip.textContent, '');
});

test('a fetch is amber while it runs, green after, and red when it fails', async () => {
  const chip = { dataset: {} };
  let during;
  await updating(chip, async () => { during = chip.dataset.state; });
  assert.equal(during, 'wait');
  assert.equal(chip.dataset.state, 'live');
  await updating(chip, async () => { throw new Error('no connection'); });
  assert.equal(chip.dataset.state, 'off');
});

test('the dot is a button, and Refresh is no longer on the bar', () => {
  assert.match(page, /<button id="live" type="button" hidden><\/button>/);
  assert.ok(!page.includes('b-refresh'));
});

test('Hide and Isolate sit in the Visibility fly-out, Show all is gone for the quiet ✕, and Properties has left the bar', () => {
  assert.ok(bar.includes('id="b-vis"'));
  for (const id of ['b-hide', 'b-isolate', 'b-props']) assert.ok(!bar.includes(`id="${id}"`), id);
  for (const id of ['b-hide', 'b-isolate']) assert.ok(pop.includes(`id="${id}"`), id);
  assert.ok(!page.includes('b-showall'));
  assert.match(page, /<button id="quiet" type="button" hidden>/);
});
