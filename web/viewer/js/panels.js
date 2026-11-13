// The floating panels: dragged by their name line, sized by any edge or corner, kept whole inside the model, and the groups
// that fold in them. The numbers are plain functions, so `web/tests/panels.test.mjs` runs them with no browser. On a phone the
// panels are bottom sheets, so none of this applies there.

export const MIN = { w: 220, h: 140 };   // A panel's least size, in pixels.
export const GAP = 4;                    // What a panel keeps from the model's edge.
const PHONE = 700;
const clamp = (v, a, b) => Math.max(a, Math.min(v, b));

// A box {x, y, w, h} moved and shrunk until it is whole inside an area {w, h}: never under the least size.
export function placed(b, area, min = MIN) {
  const w = clamp(b.w, min.w, Math.max(min.w, area.w - 2 * GAP)), h = clamp(b.h, min.h, Math.max(min.h, area.h - 2 * GAP));
  return { x: clamp(b.x, GAP, Math.max(GAP, area.w - w - GAP)), y: clamp(b.y, GAP, Math.max(GAP, area.h - h - GAP)), w, h };
}

// Where the middle of a fly-out `width` wide goes to stand over a button's `middle`, whole inside an area `area` wide.
export const overButton = (middle, width, area) => clamp(middle, width / 2 + GAP, Math.max(width / 2 + GAP, area - width / 2 - GAP));

// A box with an edge or corner ('n', 'e', 's', 'w', 'ne', ...) pulled by (dx, dy). The far edges stay where they are.
export function resized(s, side, dx, dy, area, min = MIN) {
  let l = s.x, t = s.y, r = s.x + s.w, b = s.y + s.h;
  if (side.includes('w')) l = clamp(l + dx, GAP, r - min.w);
  if (side.includes('e')) r = clamp(r + dx, l + min.w, Math.max(l + min.w, area.w - GAP));
  if (side.includes('n')) t = clamp(t + dy, GAP, b - min.h);
  if (side.includes('s')) b = clamp(b + dy, t + min.h, Math.max(t + min.h, area.h - GAP));
  return { x: l, y: t, w: r - l, h: b - t };
}

// The width of the picture no open panel stands over: a panel holds the side its middle is on, wherever it was dragged to.
export function clearOf(rects, width) {
  const f = { x0: 0, x1: width };
  for (const r of rects) {
    if ((r.left + r.right) / 2 < width / 2) f.x0 = Math.max(f.x0, r.right + 8);
    else f.x1 = Math.min(f.x1, r.left - 8);
  }
  return f;
}

// A group that folds: its title, then its rows, sliding shut as CSS has it (`.rows`). `changed` hears each fold.
export function fold(title, kids, shut, changed) {
  const b = document.createElement('button'), box = document.createElement('div'), inner = document.createElement('div');
  b.type = 'button'; b.className = 'fold'; b.setAttribute('aria-expanded', String(!shut));
  b.innerHTML = '<svg viewBox="0 0 10 10" aria-hidden="true"><path d="M1 2.5h8L5 8z"/></svg>'; b.append(title);
  inner.className = 'in'; inner.append(...kids);
  box.className = shut ? 'rows shut' : 'rows'; box.append(inner);
  b.onclick = () => { const now = box.classList.toggle('shut'); b.setAttribute('aria-expanded', String(!now)); changed?.(now); };
  return [b, box];
}

// Makes a panel move and size inside `view`. `done` hears each change. What it returns keeps the panel inside after the
// view changes size, and gives a phone's sheet back its own place.
export function movable(panel, view, done) {
  const phone = () => window.innerWidth <= PHONE;
  const area = () => ({ w: view.clientWidth, h: view.clientHeight });
  const box = () => ({ x: panel.offsetLeft, y: panel.offsetTop, w: panel.offsetWidth, h: panel.offsetHeight });
  const put = b => { Object.assign(panel.style, { left: b.x + 'px', top: b.y + 'px', width: b.w + 'px', height: b.h + 'px', right: 'auto', bottom: 'auto' }); done?.(); };
  const follow = (el, move) => el.addEventListener('pointerdown', e => {
    if (e.button || phone() || e.target.closest('.x')) return;
    e.preventDefault(); el.setPointerCapture(e.pointerId);
    const from = box(), x = e.clientX, y = e.clientY;
    const go = m => put(move(from, m.clientX - x, m.clientY - y));
    const stop = () => { el.removeEventListener('pointermove', go); el.removeEventListener('pointerup', stop); el.removeEventListener('pointercancel', stop); };
    el.addEventListener('pointermove', go); el.addEventListener('pointerup', stop); el.addEventListener('pointercancel', stop);
  });
  panel.addEventListener('pointerdown', () => {
    panel.parentElement.querySelectorAll('.panel.front').forEach(p => p.classList.remove('front'));
    panel.classList.add('front');
  });
  follow(panel.querySelector('h2'), (b, dx, dy) => placed({ ...b, x: b.x + dx, y: b.y + dy }, area()));
  for (const side of ['n', 'e', 's', 'w', 'ne', 'nw', 'se', 'sw']) {
    const h = document.createElement('div'); h.className = 'edge ' + side; panel.append(h);
    follow(h, (b, dx, dy) => resized(b, side, dx, dy, area()));
  }
  panel.insertAdjacentHTML('beforeend', '<svg class="grip" viewBox="0 0 9 9" aria-hidden="true"><path d="M8 2 2 8M8 5 5 8"/></svg>');
  return () => {
    if (!panel.style.left) return;
    if (phone()) panel.removeAttribute('style'); else if (!panel.hidden) put(placed(box(), area()));
  };
}
