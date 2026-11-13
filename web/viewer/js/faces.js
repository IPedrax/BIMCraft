// A flat face's outline, in plain numbers with no browser in it, so `web/tests/faces.test.mjs` runs it: a double-click in
// Measure shows the face's edges with their lengths and its area, as SolidWorks shows a face's sizes. A face is the triangles
// on one plane that join the one clicked edge to edge; its outline is the edges only one of them has, joined into the straight
// runs a person sees (a triangle's seam, or a corner standing in a straight edge, is not an edge).

const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const len = a => Math.sqrt(dot(a, a));

// Whether a triangle (`a`, `b`, `c`, points in the world) is on the face with way `n` (a unit vector) at `d` (n · p for a point p on
// it): its corners within 2 mm of the plane, facing the same way, and not a sliver. The underside of a sheet 1 mm thick faces the
// other way: lit with the top, it fought the top for the same pixels.
export function onFace(a, b, c, n, d) {
  for (const p of [a, b, c]) if (Math.abs(dot(n, p) - d) > 0.002) return false;
  const t = cross(sub(b, a), sub(c, a)), l = len(t);
  return l > 1e-6 && dot(t, n) / l >= 0.999;
}

// `pos`: triangles, nine numbers each (metres, in the world); `seed`: the triangle the face grows from. Corners within `tol` are
// one. Returns the outline's straight runs ([a, b], each a point [x, y, z]), the area and its middle.
export function outline(pos, seed = 0, tol = 1e-4) {
  const verts = [], ids = new Map(), tris = [];
  const vid = (x, y, z) => {
    const k = `${Math.round(x / tol)},${Math.round(y / tol)},${Math.round(z / tol)}`;
    let i = ids.get(k);
    if (i === undefined) { i = verts.length; ids.set(k, i); verts.push([x, y, z]); }
    return i;
  };
  for (let t = 0; t + 8 < pos.length; t += 9) tris.push([vid(pos[t], pos[t + 1], pos[t + 2]), vid(pos[t + 3], pos[t + 4], pos[t + 5]), vid(pos[t + 6], pos[t + 7], pos[t + 8])]);
  if (!tris[seed]) return { segments: [], area: 0, centre: null };
  const edge = (a, b) => a < b ? a + ':' + b : b + ':' + a;
  const sides = tr => [0, 1, 2].map(k => [tr[k], tr[(k + 1) % 3]]).filter(([a, b]) => a !== b);
  const byEdge = new Map();
  tris.forEach((tr, i) => sides(tr).forEach(([a, b]) => { const e = edge(a, b); if (!byEdge.has(e)) byEdge.set(e, []); byEdge.get(e).push(i); }));
  // The face: the seed and every triangle reached from it across a shared edge.
  const face = new Set([seed]), todo = [seed];
  while (todo.length) {
    for (const [a, b] of sides(tris[todo.pop()])) for (const j of byEdge.get(edge(a, b))) if (!face.has(j)) { face.add(j); todo.push(j); }
  }
  // Its outline: the edges only one of its triangles has.
  const count = new Map(), first = new Map();
  for (const i of face) for (const [a, b] of sides(tris[i])) { const e = edge(a, b); count.set(e, (count.get(e) || 0) + 1); if (!first.has(e)) first.set(e, [a, b]); }
  const bound = [...count].filter(([, n]) => n === 1).map(([e]) => first.get(e));
  const at = new Map();
  bound.forEach((e, n) => e.forEach(v => { if (!at.has(v)) at.set(v, []); at.get(v).push(n); }));
  // A point where two outline edges meet in a straight line is not a corner, and the run goes on through it.
  const corner = v => {
    const es = at.get(v);
    if (es.length !== 2) return true;
    const [d1, d2] = es.map(n => { const e = bound[n]; return sub(verts[e[0] === v ? e[1] : e[0]], verts[v]); });
    const l = len(d1) * len(d2);
    return !(l > 0) || len(cross(d1, d2)) / l > 1e-4 || dot(d1, d2) > 0;
  };
  const used = new Set(), segments = [];
  const extend = v => {
    while (!corner(v)) {
      const next = at.get(v).find(m => !used.has(m)); if (next === undefined) break;
      used.add(next); const e = bound[next]; v = e[0] === v ? e[1] : e[0];
    }
    return v;
  };
  bound.forEach((e, n) => {
    if (used.has(n)) return;
    used.add(n);
    const a = extend(e[0]), b = extend(e[1]);
    if (a !== b) segments.push([verts[a], verts[b]]);
  });
  let area = 0; const c = [0, 0, 0];
  for (const i of face) {
    const [p, q, r] = tris[i].map(k => verts[k]), s = len(cross(sub(q, p), sub(r, p))) / 2;
    area += s; for (let k = 0; k < 3; k++) c[k] += s * (p[k] + q[k] + r[k]) / 3;
  }
  return { segments, area, centre: area > 0 ? c.map(v => v / area) : verts[tris[seed][0]] };
}
