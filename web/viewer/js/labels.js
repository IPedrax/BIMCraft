// Where the measure labels go on the screen this frame, in plain numbers with no browser in it, so `web/tests/labels.test.mjs`
// runs it. Labels are laid again every frame, so as the view moves each stays readable: the most wanted first, each at the first
// of its places that is on the screen and clear of those already laid, else (one that must show) at its first place, or (one that
// may go) not at all.
//
// A label: `x`, `y` its anchor and `w`, `h` its size in pixels, `prio` (lower is laid first), `tries` the offsets from the anchor
// to try in turn ([dx, dy]), `must` whether it shows even where it overlaps. `box` is the screen: { x0, y0, x1, y1 }.
export function layout(labels, box, pad = 3) {
  const laid = [], out = labels.map(() => null);
  const order = labels.map((_, i) => i).sort((a, b) => labels[a].prio - labels[b].prio || a - b);
  const rect = (l, [dx, dy]) => ({ x0: l.x + dx - l.w / 2 - pad, y0: l.y + dy - l.h / 2 - pad, x1: l.x + dx + l.w / 2 + pad, y1: l.y + dy + l.h / 2 + pad });
  const onScreen = r => r.x0 >= box.x0 && r.y0 >= box.y0 && r.x1 <= box.x1 && r.y1 <= box.y1;
  const clear = r => laid.every(q => r.x1 <= q.x0 || r.x0 >= q.x1 || r.y1 <= q.y0 || r.y0 >= q.y1);
  for (const i of order) {
    const l = labels[i], tries = l.tries?.length ? l.tries : [[0, 0]];
    let t = tries.find(d => { const r = rect(l, d); return onScreen(r) && clear(r); });
    if (!t && l.must) t = tries[0];
    if (!t) continue;
    laid.push(rect(l, t)); out[i] = { x: l.x + t[0], y: l.y + t[1] };
  }
  return out;
}

// The places to try for a label on a line seen from `a` to `b` (screen points): on it, then off it either side, then along it.
export function along(a, b, w, h) {
  const dx = b.x - a.x, dy = b.y - a.y, l = Math.hypot(dx, dy) || 1, ux = dx / l, uy = dy / l, px = -uy, py = ux;
  const side = Math.abs(px) * w / 2 + Math.abs(py) * h / 2 + 6, run = Math.abs(ux) * w / 2 + Math.abs(uy) * h / 2 + 8;
  return [[0, 0], [px * side, py * side], [-px * side, -py * side], [ux * run, uy * run], [-ux * run, -uy * run]];
}

// The places for a tag beside a line from `a` to `b`, on the side away from `away` first: off the line, never on it, by its
// middle and then a quarter of the way from either end, so a label in the middle of a face does not push it out.
export function beside(a, b, w, h, away) {
  const dx = b.x - a.x, dy = b.y - a.y, l = Math.hypot(dx, dy) || 1, mx = (a.x + b.x) / 2, my = (a.y + b.y) / 2;
  let px = -dy / l, py = dx / l;
  if (away && (mx - away.x) * px + (my - away.y) * py < 0) { px = -px; py = -py; }
  const side = Math.abs(px) * w / 2 + Math.abs(py) * h / 2 + 5, out = [];
  for (const k of [0, -0.25, 0.25]) for (const sd of [1, -1]) out.push([px * side * sd + dx * k, py * side * sd + dy * k]);
  return out;
}
