// What the right-click menu offers (`web/viewer/js/menu.js`), run by `node --test web/tests/menu.test.mjs`.
import test from 'node:test';
import assert from 'node:assert/strict';
import { rows, quietLabel } from '../viewer/js/menu.js';

test('on an element in the 3D view the menu holds what it always did', () => {
  assert.deepEqual(rows({ picked: true, selected: false }),
    ['Select', 'Properties', 'Focus', 'Hide', 'Isolate', null, 'Transparency', null, 'Home view']);
});

test('no row says Show all: Exit isolate while isolated, Show hidden while something is hidden, neither when nothing is', () => {
  assert.ok(!rows({ picked: true, hidden: 3 }).includes('Show all'));
  assert.deepEqual(rows({ isolated: true, hidden: 40 }), ['Transparency', null, 'Exit isolate', 'Home view']);
  assert.deepEqual(rows({ hidden: 3 }), ['Transparency', null, 'Show hidden', 'Home view']);
  assert.deepEqual(rows({ plan: true, isolated: true }), ['Exit isolate']);
});

test('the quiet ✕ says Exit isolate (with Esc) while isolated, else how many are hidden, and is gone when nothing is', () => {
  assert.deepEqual(quietLabel({ isolated: true, hidden: 12 }), { text: 'Exit isolate', key: true });
  assert.deepEqual(quietLabel({ hidden: 3 }), { text: 'Show {} hidden', n: 3 });
  assert.equal(quietLabel({}), null);
  assert.equal(quietLabel({ hidden: 0 }), null);
});

test('off the model, the selection still has its Properties, Hide and Isolate, since the bar no longer does', () => {
  const r = rows({ picked: false, selected: true });
  for (const row of ['Properties', 'Focus', 'Hide', 'Isolate', 'Clear selection']) assert.ok(r.includes(row), row);
  assert.ok(!r.includes('Select'));
});

test('off the model with nothing selected, nothing is offered for an element', () => {
  assert.deepEqual(rows({ picked: false, selected: false }), ['Transparency', null, 'Home view']);
});

test('a plan or sheet offers what works there, and nothing that needs the 3D view', () => {
  assert.deepEqual(rows({ picked: true, selected: false, plan: true }), ['Select', 'Properties', 'Hide', 'Isolate']);
  assert.deepEqual(rows({ picked: false, selected: false, plan: true }), []);
  assert.deepEqual(rows({ picked: false, selected: true, plan: true }), ['Properties', 'Hide', 'Isolate', null, 'Clear selection']);
});
