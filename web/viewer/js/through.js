// See Through: for now, and only in the 3D view, whole categories and picked elements go clear, a click takes what stands
// behind them, and Tab steps back through what is under the pointer. The model and the file stay as they were, and walls
// still stop a walker: it changes how things are drawn and picked, nothing else.
export const LEVEL = 70, MIN_LEVEL = 5, MAX_LEVEL = 95;
// What goes clear until someone says otherwise: what stands between the eye and the services.
export const CATEGORIES = ['wall', 'floor', 'ceiling', 'roof'];

export const settings = () => ({ on: false, level: LEVEL, categories: new Set(CATEGORIES), picked: new Set() });
// A percent kept where a wall can still be seen; what is no number leaves `was` as it stands.
export function level(v, was = LEVEL) {
  const n = typeof v === 'string' && v.trim() === '' ? NaN : Number(v);
  return Number.isFinite(n) ? Math.min(MAX_LEVEL, Math.max(MIN_LEVEL, n)) : was;
}
// How solid something is drawn: on top of what it was, so glass stays clearer than a wall.
export const opacity = (percent, was = 1) => was * (1 - percent / 100);
export const clears = (s, category, key) => s.on && (s.categories.has(category) || s.picked.has(key));
// The next of what stands under the pointer (front first, an element once for each of its faces) after `current`.
export function behind(under, current) {
  const list = [...new Set(under)];
  return list.length ? list[(list.indexOf(current) + 1) % list.length] : null;
}

// The menu's rows from off start afresh, with only what was chosen; while on, they add and take away.
function fresh(s) { if (!s.on) { s.on = true; s.categories.clear(); s.picked.clear(); } }
const settle = s => { if (!s.categories.size && !s.picked.size) s.on = false; };
const flip = (set, x) => { if (set.has(x)) set.delete(x); else set.add(x); };
export function choose(s, category) { fresh(s); flip(s.categories, category); settle(s); }
export function pick(s, key) { fresh(s); flip(s.picked, key); settle(s); }
// On and off with the key: what was clear comes back, or the four categories.
export function toggle(s) {
  s.on = !s.on;
  if (s.on && !s.categories.size && !s.picked.size) CATEGORIES.forEach(c => s.categories.add(c));
}

const key = n => n.userData.guid ?? n.userData.id;
const LABELS = { wall: 'Walls', floor: 'Floors', ceiling: 'Ceilings', roof: 'Roofs' };

// The rows of the right-click menu's Transparency, the bar of how clear, and Tab and X. The drawing is `app.through` in `main.js`.
export class Through {
  constructor(app) {
    this.app = app; this.s = settings(); this.at = null;
    app.clears = n => clears(this.s, n.userData.category, key(n));
    const get = id => document.getElementById(id);
    this.bar = get('thrubar'); this.range = get('thru-level'); this.out = get('thru-out');
    this.range.oninput = () => { this.s.level = level(this.range.value, this.s.level); this.show(); app.through(); };
    const canvas = app.renderer.domElement;
    canvas.addEventListener('pointermove', e => { this.at = [e.clientX, e.clientY]; });
    canvas.addEventListener('pointerleave', () => { this.at = null; });
    // The bar goes while the view moves, and comes back a moment after.
    let still = 0;
    app.on('camera', () => {
      this.bar.classList.add('moving'); clearTimeout(still);
      still = setTimeout(() => this.bar.classList.remove('moving'), 700);
    });
    this.show();
  }
  show() {
    this.bar.hidden = !this.s.on; this.range.value = this.s.level; this.out.textContent = this.s.level + ' %';
  }
  sync() { this.show(); this.app.through(); }
  // The rows of the menu, `n` being what Selection takes: the selected element, else what was right-clicked.
  rows(n) {
    const s = this.s, app = this.app;
    return [
      ...CATEGORIES.map(c => ({ label: LABELS[c], on: s.on && s.categories.has(c), act: () => { choose(s, c); this.sync(); } })),
      null,
      { label: 'Selection', on: !!n && s.on && s.picked.has(key(n)), off: !n, act: () => {
        pick(s, key(n));
        if (app.selected === n && s.picked.has(key(n))) app.select(null); // A selected element is drawn solid.
        this.sync();
      } },
      null,
      { label: 'Off', off: !s.on, act: () => { s.on = false; this.sync(); } },
    ];
  }
  // X turns it on and off; Tab, while it is on, takes the next thing back under the pointer, clear or not. Whether it took the key.
  key(e) {
    if (e.ctrlKey || e.metaKey || e.altKey) return false;
    if (e.key === 'x' || e.key === 'X') { toggle(this.s); this.sync(); return true; }
    if (e.key !== 'Tab' || !this.s.on || !this.at) return false;
    const next = behind(this.app.under(...this.at), this.app.selected);
    if (next) this.app.select(next);
    return !!next;
  }
}
