// What the right-click menu offers, in plain names with null for a line, so `web/tests/menu.test.mjs` runs it with no browser. What
// each row does is `main.js`'s. A row for an element goes on the one under the pointer, or else on the selection: the tool bar has no
// Properties, so an element picked from the model browser, off screen, gets them here. A plan or sheet has no Focus,
// Transparency or Home view: those need the 3D view. There is no Show all: what Isolate hid comes back with Exit isolate, and what
// Hide hid with Show hidden, each offered only while it has something to do.
export function rows({ picked = false, selected = false, plan = false, isolated = false, hidden = 0 }) {
  const element = picked || selected, list = [];
  if (picked) list.push('Select');
  if (element) list.push('Properties', ...(plan ? [] : ['Focus']), 'Hide', 'Isolate', null);
  if (!plan) list.push('Transparency', null);
  if (isolated) list.push('Exit isolate'); else if (hidden) list.push('Show hidden');
  if (selected) list.push('Clear selection');
  if (!plan) list.push('Home view');
  while (list.length && list[list.length - 1] === null) list.pop();
  return list;
}

// What the quiet ✕ at the top of the view says, or null when there is nothing to undo: Exit isolate (Esc does it too) while
// isolated, else how many are hidden. `text` is English for `t`/`tf`; `key` says Esc does it.
export function quietLabel({ isolated = false, hidden = 0 }) {
  if (isolated) return { text: 'Exit isolate', key: true };
  if (hidden > 0) return { text: 'Show {} hidden', n: hidden };
  return null;
}
