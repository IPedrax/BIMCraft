// What Measure snaps to under the pointer, in plain numbers with no browser in it, so `web/tests/snaps.test.mjs` runs it. The
// pointer takes, within 12 px, the nearest of a corner, an edge's midpoint, a circle's centre or a place where two edges cross on
// the screen (a corner on a tie); else, within 8 px, the nearest point of an edge; else the face. Each kind is shown by its own
// shape (`snapMark`): a square, a triangle, a circle with a cross, an X, a solid bow tie along the edge and a plus. The Autodesk viewer
// marks a point snap with a square and an edge snap with a cross of three lines (its developer's guide, Edit2D, "Snapping");
// these shapes are our own, one per kind.
//
// Points are [x, y, z] in the world (metres). The edges are a flat face's outline as `faces.js` gives it: straight runs, and a
// round edge as many short sides. `toScreen(p)` is { x, y, z, behind } in px, z the depth (nearer is smaller).

export const MAX_LOOP = 4096; // sides in a loop searched for a circle
const MAX_SEGMENTS = 20000; // edges looked at under one pointer
const POINT_PX = 12, EDGE_PX = 8;

const fin = p => Array.isArray(p) && p.length >= 3 && Number.isFinite(p[0]) && Number.isFinite(p[1]) && Number.isFinite(p[2]);
const okSeg = s => Array.isArray(s) && fin(s[0]) && fin(s[1]);
const segs = list => (Array.isArray(list) ? list.slice(0, MAX_SEGMENTS).filter(okSeg) : []);
const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const add = (a, b) => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const mul = (a, k) => [a[0] * k, a[1] * k, a[2] * k];
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross3 = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const len = a => Math.sqrt(dot(a, a));
const lerp = (a, b, t) => add(a, mul(sub(b, a), t));
const keyOf = (p, tol) => `${Math.round(p[0] / tol)},${Math.round(p[1] / tol)},${Math.round(p[2] / tol)}`;

// The closed loops the edges make, each its points in order. Edges that make no closed loop are left out.
export function loops(segments, tol = 1e-4) {
  const list = segs(segments), at = new Map(), pt = new Map();
  list.forEach((s, n) => s.forEach(p => {
    const k = keyOf(p, tol);
    if (!at.has(k)) { at.set(k, []); pt.set(k, p); }
    at.get(k).push(n);
  }));
  const used = new Set(), out = [];
  list.forEach((s, n) => {
    if (used.has(n)) return;
    const start = keyOf(s[0], tol), pts = [pt.get(start)];
    let k = keyOf(s[1], tol), m = n;
    used.add(n);
    while (k !== start && pts.length <= MAX_SEGMENTS) {
      pts.push(pt.get(k));
      const next = at.get(k).find(j => !used.has(j));
      if (next === undefined) { m = -1; break; }
      used.add(next); m = next;
      const e = list[next], k0 = keyOf(e[0], tol);
      k = k0 === k ? keyOf(e[1], tol) : k0;
    }
    if (m !== -1 && k === start && pts.length >= 3) out.push(pts);
  });
  return out;
}

// A loop that is a circle: { centre, radius, normal }, else null. At least 8 sides, none turning more than 45 degrees, every point
// within 1 % of the radius (0.5 mm at least) of the circle and of its plane.
export function circle(pts) {
  if (!Array.isArray(pts) || pts.length < 8 || pts.length > MAX_LOOP || !pts.every(fin)) return null;
  const n = pts.length, c = mul(pts.reduce(add, [0, 0, 0]), 1 / n);
  // The plane's way, by Newell's sum.
  let w = [0, 0, 0];
  for (let i = 0; i < n; i++) {
    const a = pts[i], b = pts[(i + 1) % n];
    w = add(w, [(a[1] - b[1]) * (a[2] + b[2]), (a[2] - b[2]) * (a[0] + b[0]), (a[0] - b[0]) * (a[1] + b[1])]);
  }
  const wl = len(w); if (!(wl > 0)) return null;
  const nz = mul(w, 1 / wl);
  const u0 = Math.abs(nz[0]) < 0.9 ? [1, 0, 0] : [0, 1, 0];
  const u = (() => { const v = cross3(nz, u0); return mul(v, 1 / len(v)); })(), v = cross3(nz, u);
  const xy = pts.map(p => { const d = sub(p, c); return [dot(d, u), dot(d, v), dot(d, nz)]; });
  // x² + y² + Dx + Ey + F = 0, fitted by least squares.
  let sxx = 0, sxy = 0, syy = 0, sx = 0, sy = 0, sxz = 0, syz = 0, sz = 0;
  for (const [x, y] of xy) {
    const z = -(x * x + y * y);
    sxx += x * x; sxy += x * y; syy += y * y; sx += x; sy += y; sxz += x * z; syz += y * z; sz += z;
  }
  const sol = solve3([[sxx, sxy, sx], [sxy, syy, sy], [sx, sy, n]], [sxz, syz, sz]);
  if (!sol) return null;
  const [D, E, F] = sol, cx = -D / 2, cy = -E / 2, r2 = cx * cx + cy * cy - F;
  if (!(r2 > 0)) return null;
  const r = Math.sqrt(r2), tol = Math.max(0.0005, 0.01 * r), side = 2 * r * Math.sin(Math.PI / 8) * 1.0001;
  for (let i = 0; i < n; i++) {
    const [x, y, z] = xy[i], [x1, y1, z1] = xy[(i + 1) % n];
    if (Math.abs(Math.hypot(x - cx, y - cy) - r) > tol || Math.abs(z) > tol) return null;
    if (Math.hypot(x1 - x, y1 - y, z1 - z) > side) return null;
  }
  return { centre: add(c, add(mul(u, cx), mul(v, cy))), radius: r, normal: nz };
}

function solve3(m, b) {
  const a = m.map((row, i) => [...row, b[i]]);
  for (let col = 0; col < 3; col++) {
    let piv = col;
    for (let r = col + 1; r < 3; r++) if (Math.abs(a[r][col]) > Math.abs(a[piv][col])) piv = r;
    if (!(Math.abs(a[piv][col]) > 1e-18)) return null;
    [a[col], a[piv]] = [a[piv], a[col]];
    for (let r = 0; r < 3; r++) if (r !== col) {
      const f = a[r][col] / a[col][col];
      for (let k = col; k < 4; k++) a[r][k] -= f * a[col][k];
    }
  }
  const x = [0, 1, 2].map(i => a[i][3] / a[i][i]);
  return x.every(Number.isFinite) ? x : null;
}

// The circles the edges make, and the points that are theirs (so a circle's sides are neither corners nor straight edges).
function rounds(segments, tol = 1e-4) {
  const circles = [], keys = new Set();
  for (const l of loops(segments, tol)) {
    const c = circle(l);
    if (c) { circles.push(c); l.forEach(p => keys.add(keyOf(p, tol))); }
  }
  return { circles, keys };
}
const straight = (segments, keys, tol = 1e-4) => segs(segments).filter(s => !(keys.has(keyOf(s[0], tol)) && keys.has(keyOf(s[1], tol))));

// The middle of each straight edge.
export function midpoints(segments) {
  const { keys } = rounds(segments);
  return straight(segments, keys).map(([a, b]) => ({ p: lerp(a, b, 0.5), a, b }));
}

// Where straight edges end; a circle's points are not corners.
export function corners(segments, tol = 1e-4) {
  const { keys } = rounds(segments, tol), seen = new Map();
  for (const s of straight(segments, keys, tol)) for (const p of s) { const k = keyOf(p, tol); if (!seen.has(k)) seen.set(k, p); }
  return [...seen.values()];
}

const okAt = at => at && Number.isFinite(at.x) && Number.isFinite(at.y);
const okScreen = s => s && !s.behind && Number.isFinite(s.x) && Number.isFinite(s.y);
const segPx = (s, at) => {
  const dx = s.b.x - s.a.x, dy = s.b.y - s.a.y, l2 = dx * dx + dy * dy;
  const t = l2 > 0 ? Math.max(0, Math.min(1, ((at.x - s.a.x) * dx + (at.y - s.a.y) * dy) / l2)) : 0;
  return { t, d: Math.hypot(s.a.x + t * dx - at.x, s.a.y + t * dy - at.y) };
};
// The world point of segment `w` (ends in the world) whose picture is at screen parameter `t` of its picture: found by halving,
// as a perspective view does not carry a line's parameter over evenly.
function onWorld(w, t, toScreen) {
  const A = toScreen(w[0]), B = toScreen(w[1]), dx = B.x - A.x, dy = B.y - A.y, l2 = dx * dx + dy * dy;
  if (!(l2 > 0)) return w[0];
  let lo = 0, hi = 1;
  for (let i = 0; i < 48; i++) {
    const mid = (lo + hi) / 2, s = toScreen(lerp(w[0], w[1], mid));
    if (((s.x - A.x) * dx + (s.y - A.y) * dy) / l2 < t) lo = mid; else hi = mid;
  }
  return lerp(w[0], w[1], (lo + hi) / 2);
}
// The edges' pictures that come within `r` px of the pointer.
function seen(segments, at, toScreen, r) {
  const out = [];
  for (const w of segs(segments)) {
    const a = toScreen(w[0]), b = toScreen(w[1]);
    if (!okScreen(a) || !okScreen(b)) continue;
    const s = { w, a, b };
    if (segPx(s, at).d <= r) out.push(s);
  }
  return out;
}

// Where two edges cross on the screen within `r` px of the pointer, the nearest: { p, x, y }, p on the edge in front. Two edges
// that only meet at their ends make a corner, not a crossing.
export function crossing(segments, at, toScreen, r = POINT_PX) {
  if (!okAt(at) || typeof toScreen !== 'function') return null;
  const ss = seen(segments, at, toScreen, r).slice(0, 256);
  let best = null, bd = r;
  for (let i = 0; i < ss.length; i++) for (let j = i + 1; j < ss.length; j++) {
    const p = ss[i], q = ss[j];
    const rx = p.b.x - p.a.x, ry = p.b.y - p.a.y, sx = q.b.x - q.a.x, sy = q.b.y - q.a.y, den = rx * sy - ry * sx;
    const lp = Math.hypot(rx, ry), lq = Math.hypot(sx, sy);
    if (!(Math.abs(den) > 1e-9 * lp * lq) || !(lp > 0) || !(lq > 0)) continue;
    const ox = q.a.x - p.a.x, oy = q.a.y - p.a.y, t = (ox * sy - oy * sx) / den, u = (ox * ry - oy * rx) / den;
    if (t < 0 || t > 1 || u < 0 || u > 1) continue;
    const endP = Math.min(t, 1 - t) * lp < 1.5, endQ = Math.min(u, 1 - u) * lq < 1.5;
    if (endP && endQ) continue;
    const x = p.a.x + t * rx, y = p.a.y + t * ry, d = Math.hypot(x - at.x, y - at.y);
    if (!(d <= bd)) continue;
    const P = onWorld(p.w, t, toScreen), Q = onWorld(q.w, u, toScreen);
    const zp = toScreen(P).z, zq = toScreen(Q).z;
    bd = d; best = { p: (zq < zp ? Q : P), x, y };
  }
  return best;
}

const ORDER = ['corner', 'midpoint', 'centre', 'crossing'];
// What the pointer `at` (px) snaps to on `face` ({ point: where the ray hit, edges: the face's outline, more: edges of the faces
// beside it }): { p, kind, angle } (angle: an edge's way on the screen, in radians), or null with no face.
export function snapAt(at, face, toScreen, opts = {}) {
  if (!face || !fin(face.point)) return null;
  const out = { p: face.point, kind: 'face', angle: 0 };
  if (!okAt(at) || typeof toScreen !== 'function') return out;
  const pointPx = opts.point ?? POINT_PX, edgePx = opts.edge ?? EDGE_PX, edges = segs(face.edges), more = segs(face.more);
  const { circles } = rounds(edges);
  const cands = [
    ...corners(edges).map(p => ({ p, kind: 'corner' })),
    ...midpoints(edges).map(m => ({ p: m.p, kind: 'midpoint' })),
    ...circles.map(c => ({ p: c.centre, kind: 'centre' })),
  ];
  const x = crossing([...edges, ...more], at, toScreen, pointPx);
  if (x) cands.push({ p: x.p, kind: 'crossing' });
  let best = null, bd = Infinity;
  for (const c of cands) {
    const s = toScreen(c.p); if (!okScreen(s)) continue;
    const d = Math.hypot(s.x - at.x, s.y - at.y);
    if (d > pointPx) continue;
    if (d < bd - 0.5 || (Math.abs(d - bd) <= 0.5 && ORDER.indexOf(c.kind) < ORDER.indexOf(best.kind))) { bd = Math.min(bd, d); best = c; }
  }
  if (best) return { p: best.p, kind: best.kind, angle: 0 };
  let ed = edgePx, edge = null;
  for (const s of seen(edges, at, toScreen, edgePx)) {
    const { t, d } = segPx(s, at);
    if (d <= ed) { ed = d; edge = { p: onWorld(s.w, t, toScreen), kind: 'edge', angle: Math.atan2(s.b.y - s.a.y, s.b.x - s.a.x) }; }
  }
  return edge || out;
}

const f = v => v.toFixed(1);
// The shape that shows a snap of `kind` at (x, y) px, as SVG: `ink` strokes `width` px on a `halo` `haloW` px wider; `angle` turns
// an edge's bow tie along its edge.
export function snapMark(kind, x, y, angle = 0, colours = {}) {
  if (!Number.isFinite(x) || !Number.isFinite(y) || !ORDER.includes(kind) && kind !== 'edge' && kind !== 'face') return '';
  const ink = colours.ink || '#1c5ca3', halo = colours.halo || '#fff', w = colours.width || 1.75, e = colours.haloW ?? 2.5;
  // A bow tie turned to a slant reads as a zig-zag: it lies flat for an edge nearer level, upright for one nearer plumb.
  const a = Number.isFinite(angle) && Math.abs(Math.cos(angle)) < Math.SQRT1_2 ? Math.PI / 2 : 0;
  const stroke = (d, fill = 'none') => `<path d="${d}" fill="${fill}" stroke="${halo}" stroke-width="${w + e}" stroke-linejoin="round" stroke-linecap="round"/>`
    + `<path d="${d}" fill="${fill}" stroke="${ink}" stroke-width="${w}" stroke-linejoin="round" stroke-linecap="round"/>`;
  const P = (px, py) => `${f(px)} ${f(py)}`;
  if (kind === 'corner') { const s = 5; return stroke(`M${P(x - s, y - s)}H${f(x + s)}V${f(y + s)}H${f(x - s)}Z`); }
  if (kind === 'midpoint') { const s = 5.5; return stroke(`M${P(x, y - s * 1.1)}L${P(x + s, y + s * 0.75)}L${P(x - s, y + s * 0.75)}Z`); }
  if (kind === 'centre') {
    const r = 5.5, arm = 8;
    return stroke(`M${P(x - arm, y)}H${f(x + arm)}M${P(x, y - arm)}V${f(y + arm)}`)
      + `<circle cx="${f(x)}" cy="${f(y)}" r="${r}" fill="none" stroke="${halo}" stroke-width="${w + e}"/><circle cx="${f(x)}" cy="${f(y)}" r="${r}" fill="none" stroke="${ink}" stroke-width="${w}"/>`;
  }
  if (kind === 'crossing') { const s = 5; return stroke(`M${P(x - s, y - s)}L${P(x + s, y + s)}M${P(x + s, y - s)}L${P(x - s, y + s)}`); }
  if (kind === 'edge') {
    const s = 5.5, c = Math.cos(a), sn = Math.sin(a), R = (px, py) => P(x + px * c - py * sn, y + px * sn + py * c);
    return stroke(`M${R(-s, -s * 0.8)}L${R(s, s * 0.8)}L${R(s, -s * 0.8)}L${R(-s, s * 0.8)}Z`, ink);
  }
  const s = 5; return stroke(`M${P(x - s, y)}H${f(x + s)}M${P(x, y - s)}V${f(y + s)}`);
}
