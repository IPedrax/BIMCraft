// The viewer's words (`web/viewer/js/words.js`), run by `node --test web/tests/words.test.mjs`.
import test from 'node:test';
import assert from 'node:assert/strict';
import { t, tc, tf, useWords } from '../viewer/js/words.js';

test('with no words for a language, the English shows', () => {
  useWords(null);
  assert.equal(t('Add'), 'Add');
  assert.equal(tc('status', 'Open'), 'Open');
  assert.equal(tf('Saved {}', '9:05 PM'), 'Saved 9:05 PM');
});

test('a word shows in the language, and one the language lacks stays English', () => {
  useWords({ Add: 'Hinzufügen', 'Saved {}': 'Um {} gespeichert', 'status\u0004Open': 'Offen' });
  assert.equal(t('Add'), 'Hinzufügen');
  assert.equal(t('Cancel'), 'Cancel');
  assert.equal(tc('status', 'Open'), 'Offen');
  assert.equal(t('Open'), 'Open', 'a context is not the plain word');
  assert.equal(tf('Saved {}', '$& 9:05'), 'Um $& 9:05 gespeichert', 'a value goes in as it is');
  useWords(null);
});

test('values fill their places in order, and a word in the middle keeps the rest', () => {
  assert.equal(tf('{} by {}', 'Done', 'Sam'), 'Done by Sam');
  assert.equal(tf('{} by {}', 'Done'), 'Done by ');
});
