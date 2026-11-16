// What a section shows on screen, in plain numbers, so it needs no browser to test (`web/tests/cut.test.mjs`): where a plane
// crosses a box, what the view keeps of a flat outline, and the middle of what is left. A point is [x, y, z]; a clip plane is
// [a, b, c, d], which keeps the points with ax + by + cz + d >= 0.
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const along = (a, b, t) => a.map((v, i) => v + (b[i] - v) * t);
const AXES = { x: 0, y: 1, z: 2 };
// The twelve edges of a box, by its corners (bit 0 of a corner's number is x, bit 1 y, bit 2 z).
const EDGES = [[0, 1], [2, 3], [4, 5], [6, 7], [0, 2], [1, 3], [4, 6], [5, 7], [0, 4], [1, 5], [2, 6], [3, 7]];

// Where the plane through `p` square to `n` crosses the box: its corners in order round it, or none when it misses.
export function planeInBox(min, max, n, p) {
  const corners = EDGES.flat().map(i => [0, 1, 2].map(k => (i >> k & 1 ? max : min)[k])), side = c => dot(n, c) - dot(n, p), hits = [];
  for (let i = 0; i < corners.length; i += 2) {
    const a = corners[i], b = corners[i + 1], da = side(a), db = side(b);
    if ((da <= 0) !== (db <= 0)) hits.push(along(a, b, da / (da - db)));
  }
  if (hits.length < 3) return [];
  const mid = [0, 1, 2].map(k => hits.reduce((s, h) => s + h[k], 0) / hits.length);
  const u = cross(n, Math.abs(n[1]) > 0.9 ? [1, 0, 0] : [0, 1, 0]), v = cross(n, u);
  const angle = h => { const d = h.map((x, k) => x - mid[k]); return Math.atan2(dot(d, v), dot(d, u)); };
  return hits.sort((a, b) => angle(a) - angle(b));
}

// A face of the box: its four corners in order round it, on the low or the high side of axis `key` ('x', 'y' or 'z').
export function boxFace(min, max, key, high) {
  const k = AXES[key], u = (k + 1) % 3, v = (k + 2) % 3;
  return [[min, min], [max, min], [max, max], [min, max]].map(([a, b]) => {
    const c = [0, 0, 0]; c[k] = (high ? max : min)[k]; c[u] = a[u]; c[v] = b[v]; return c;
  });
}

// What `planes` keep of the flat outline `poly`.
export function clip(poly, planes) {
  let kept = poly;
  for (const q of planes) {
    const next = [], side = c => dot(q, c) + q[3];
    kept.forEach((a, i) => {
      const b = kept[(i + 1) % kept.length], da = side(a), db = side(b);
      if (da >= 0) next.push(a);
      if ((da < 0) !== (db < 0)) next.push(along(a, b, da / (da - db)));
    });
    kept = next;
  }
  return kept;
}

// The middle by area of the outline `pts` ([x, y]), or null when it has none, as a plane seen edge on has none.
export function middle(pts) {
  let area = 0, x = 0, y = 0;
  pts.forEach((a, i) => {
    const b = pts[(i + 1) % pts.length], k = a[0] * b[1] - b[0] * a[1];
    area += k; x += (a[0] + b[0]) * k; y += (a[1] + b[1]) * k;
  });
  return Math.abs(area) < 1e-6 ? null : [x / (3 * area), y / (3 * area)];
}

// A handle's size in the model, so that it looks the same at any zoom: a share of how far ahead of the eye it stands in a
// perspective view, of how much the view shows in a parallel one, where the eye's own place means nothing to the picture.
export const handleSize = ({ parallel, dist, depth }) => Math.max(0.002, Math.abs(parallel ? dist : depth) * 0.018);

// Whether a mesh closes round a solid, so that counting its faces finds what a plane cuts: the area vectors of the faces of a
// closed mesh sum to nought, turned out or in. `positions` is flat, x y z; `index`, when it has one, three corners a face.
export function closes(positions, index) {
  const n = index ? index.length : positions.length / 3;
  const at = i => (index ? index[i] : i) * 3;
  let area = 0;
  const sum = [0, 0, 0];
  for (let i = 0; i + 2 < n; i += 3) {
    const [a, b, c] = [at(i), at(i + 1), at(i + 2)];
    const p = [positions[a], positions[a + 1], positions[a + 2]];
    const u = [positions[b] - p[0], positions[b + 1] - p[1], positions[b + 2] - p[2]];
    const v = [positions[c] - p[0], positions[c + 1] - p[1], positions[c + 2] - p[2]];
    const f = cross(u, v);
    area += Math.hypot(...f);
    f.forEach((x, k) => { sum[k] += x; });
  }
  return area > 0 && Math.hypot(...sum) < area * 1e-4;
}
