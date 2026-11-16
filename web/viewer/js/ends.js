// How a measurement's ends are drawn, which a person picks in Measure's settings and the browser keeps: a round dot (as before,
// the default) or a small cross turned 45 degrees to the line.
import { t, tc } from './words.js';

export const END_STYLES = ['circle', 'cross'];
const KEY = 'bimcraft-viewer-measure-ends';

// The style kept in `storage` (the browser's localStorage), else the default.
// A browser that keeps nothing throws on `localStorage` itself, so it is read inside the try.
export function endStyle(storage) {
  try { const v = (storage ?? globalThis.localStorage)?.getItem(KEY); return END_STYLES.includes(v) ? v : END_STYLES[0]; } catch { return END_STYLES[0]; }
}
export function keepEnd(storage, style) {
  if (!END_STYLES.includes(style)) return;
  try { (storage ?? globalThis.localStorage)?.setItem(KEY, style); } catch { /* a private window keeps nothing */ }
}

const f = v => v.toFixed(1);
// An end at (x, y) px whose line runs out along (ux, uy), as SVG, in `ink` on a `halo` (`haloW` px wider than the stroke; a
// dot's ring `ring` px).
export function endMark(style, x, y, ux, uy, colours = {}) {
  if (!Number.isFinite(x) || !Number.isFinite(y)) return '';
  const ink = colours.ink || '#1c5ca3', halo = colours.halo || '#fff', ring = colours.ring ?? 1.5, edge = colours.haloW ?? 2.5;
  if (style !== 'cross') return `<circle cx="${f(x)}" cy="${f(y)}" r="4.5" fill="${ink}" stroke="${halo}" stroke-width="${ring}"/>`;
  let l = Math.hypot(ux, uy);
  if (!(l > 0) || !Number.isFinite(l)) { ux = 1; uy = 0; l = 1; }
  ux /= l; uy /= l;
  const c = Math.SQRT1_2, h = 4, w = colours.width || 1.75;
  const a = [(ux - uy) * c * h, (ux + uy) * c * h], b = [(ux + uy) * c * h, (uy - ux) * c * h];
  const d = `M${f(x - a[0])} ${f(y - a[1])}L${f(x + a[0])} ${f(y + a[1])}M${f(x - b[0])} ${f(y - b[1])}L${f(x + b[0])} ${f(y + b[1])}`;
  return `<path d="${d}" fill="none" stroke="${halo}" stroke-width="${w + edge}" stroke-linecap="round"/>`
    + `<path d="${d}" fill="none" stroke="${ink}" stroke-width="${w}" stroke-linecap="round"/>`;
}

// Which way each of a run of points' ends faces on the screen ({ x, y }, a unit vector): the first away from the second, every
// other away from the one before it (an angle's corner along its first arm). Along x where there is no way.
export function ways(pts) {
  if (!Array.isArray(pts)) return [];
  return pts.map((p, i) => {
    const q = i === 0 ? pts[1] : pts[i - 1], dx = p?.x - q?.x, dy = p?.y - q?.y, l = Math.hypot(dx, dy);
    return l > 0 && Number.isFinite(l) ? { x: dx / l, y: dy / l } : { x: 1, y: 0 };
  });
}

// The setting: `select` offers the styles in the page's words, shows the one kept, and keeps and passes on a new pick.
export function bindEnds(select, label, storage, changed) {
  if (!select) return;
  if (label) label.textContent = tc('measure', 'Ends');
  const names = { circle: t('Circle'), cross: tc('measure end', 'Cross') };
  select.replaceChildren(...END_STYLES.map(s => new Option(names[s], s)));
  select.value = endStyle(storage);
  select.onchange = () => { keepEnd(storage, select.value); changed?.(endStyle(storage)); };
}
