// What Measure snaps to under the pointer (`web/viewer/js/snaps.js`): a corner, an edge's midpoint, a circle's centre, where two
// edges cross on the screen, a point on an edge, or the face; the shape each is shown by; and the end style a person picks
// (`web/viewer/js/ends.js`). No browser. Run by `node --test web/tests/snaps.test.mjs`.
import test from 'node:test';
import assert from 'node:assert/strict';
import { loops, circle, midpoints, corners, crossing, snapAt, snapMark, MAX_LOOP } from '../viewer/js/snaps.js';
import { END_STYLES, endStyle, keepEnd, endMark, ways } from '../viewer/js/ends.js';
import { readFileSync } from 'node:fs';

const near = (a, b, tol = 1e-9) => assert.ok(Math.abs(a - b) <= tol, `${a} is not ${b}`);
const nearP = (p, q, tol = 1e-9) => p.forEach((v, i) => near(v, q[i], tol));
// A ring of n points round `c` in the plane z = c[2], as a face's outline gives it: one segment per side.
const ring = (c, r, n, turn = 0) => {
  const pts = Array.from({ length: n }, (_, k) => { const a = turn + 2 * Math.PI * k / n; return [c[0] + r * Math.cos(a), c[1] + r * Math.sin(a), c[2]]; });
  return pts.map((p, k) => [p, pts[(k + 1) % n]]);
};
const rect = (x0, y0, x1, y1, z = 0) => [[[x0, y0, z], [x1, y0, z]], [[x1, y0, z], [x1, y1, z]], [[x1, y1, z], [x0, y1, z]], [[x0, y1, z], [x0, y0, z]]];
// The screen of these tests: a plan from above, 1 px a millimetre, x right and y down, depth the height below 10 m.
const flat = p => ({ x: p[0] * 1000, y: -p[1] * 1000, z: 10 - p[2], behind: false });

test('segments in any order and either way round join into their closed loops', () => {
  const segs = [...ring([0, 0, 0], 0.05, 12), ...rect(1, 1, 2, 2)];
  const mixed = segs.map((s, k) => (k % 3 ? s : [s[1], s[0]])).reverse();
  const ls = loops(mixed);
  assert.equal(ls.length, 2);
  assert.deepEqual(ls.map(l => l.length).sort((a, b) => a - b), [4, 12]);
});

test('a loop of evenly spaced points on a circle is a circle: its centre, radius and the way it faces', () => {
  const c = circle(loops(ring([1, 2, 3], 0.05, 24))[0]);
  assert.ok(c, 'a 24-sided ring is a circle');
  nearP(c.centre, [1, 2, 3], 1e-9);
  near(c.radius, 0.05, 1e-9);
  near(Math.abs(c.normal[2]), 1, 1e-9);
});

test('a circle tilted out of every axis plane is still one', () => {
  const n = 16, u = [1 / Math.sqrt(2), 0, 1 / Math.sqrt(2)], v = [0, 1, 0], o = [0.3, -0.2, 1.5];
  const pts = Array.from({ length: n }, (_, k) => { const a = 2 * Math.PI * k / n; return o.map((x, i) => x + 0.1 * (Math.cos(a) * u[i] + Math.sin(a) * v[i])); });
  const c = circle(pts);
  assert.ok(c);
  nearP(c.centre, o, 1e-9);
  near(c.radius, 0.1, 1e-9);
});

test('a square, a hexagon and an ellipse are not circles: too few sides, or not round', () => {
  assert.equal(circle(loops(rect(0, 0, 1, 1))[0]), null);
  assert.equal(circle(loops(ring([0, 0, 0], 1, 6))[0]), null);
  const ellipse = Array.from({ length: 32 }, (_, k) => { const a = 2 * Math.PI * k / 32; return [0.2 * Math.cos(a), 0.1 * Math.sin(a), 0]; });
  assert.equal(circle(ellipse), null);
});

test('an edge midpoint is the middle of each straight edge, and a circle\'s sides give none', () => {
  const segs = [...rect(0, 0, 2, 1), ...ring([1, 0.5, 0], 0.1, 24)];
  const ms = midpoints(segs).map(m => m.p.map(v => +v.toFixed(9)));
  assert.equal(ms.length, 4);
  assert.deepEqual(ms.sort((a, b) => a[0] - b[0] || a[1] - b[1]), [[0, 0.5, 0], [1, 0, 0], [1, 1, 0], [2, 0.5, 0]]);
});

test('a corner is where straight edges end; the points round a circle are not corners', () => {
  const cs = corners([...rect(0, 0, 2, 1), ...ring([1, 0.5, 0], 0.1, 24)]);
  assert.equal(cs.length, 4);
});

test('two edges crossing on the screen: the crossing near the pointer, on the edge in front', () => {
  // One edge at height 2 m running east, one at 1 m running north; they cross at (0.5, 0) seen from above.
  const front = [[0, 0, 2], [1, 0, 2]], back = [[0.5, -1, 1], [0.5, 1, 1]];
  const x = crossing([front, back], { x: 505, y: 4 }, flat, 12);
  assert.ok(x);
  nearP(x.p, [0.5, 0, 2], 1e-9);
  assert.equal(crossing([front, back], { x: 530, y: 0 }, flat, 12), null, '30 px away is too far');
});

test('edges that only meet at their ends do not cross; a T where one ends on the other does', () => {
  const l = [[[0, 0, 0], [1, 0, 0]], [[1, 0, 0], [1, 1, 0]]];
  assert.equal(crossing(l, { x: 1000, y: 0 }, flat, 12), null);
  const t = [[[0, 0, 0], [1, 0, 0]], [[0.5, 0, 0], [0.5, 1, 0]]];
  const x = crossing(t, { x: 502, y: -1 }, flat, 12);
  assert.ok(x);
  nearP(x.p, [0.5, 0, 0], 1e-9);
});

test('what the pointer snaps to: a point within 12 px, nearest first, a corner on a tie; else an edge within 8 px; else the face', () => {
  const face = { point: [0.6, 0.4, 0], edges: rect(0, 0, 2, 1), more: [] };
  // A corner 5 px away.
  assert.equal(snapAt({ x: 4, y: -3 }, face, flat).kind, 'corner');
  // The bottom edge's midpoint 3 px away.
  const m = snapAt({ x: 1002, y: -3 }, face, flat);
  assert.equal(m.kind, 'midpoint'); nearP(m.p, [1, 0, 0]);
  // On the bottom edge, far from its ends and middle: the nearest point of the edge.
  const e = snapAt({ x: 600, y: -5 }, face, flat);
  assert.equal(e.kind, 'edge'); nearP(e.p, [0.6, 0, 0], 1e-12);
  // In the middle of the face.
  const f = snapAt({ x: 600, y: -400 }, face, flat);
  assert.equal(f.kind, 'face'); nearP(f.p, [0.6, 0.4, 0]);
});

test('a round hole\'s centre is snapped to, and its rim is an edge, not a row of corners', () => {
  const face = { point: [0.5, 0.5, 0], edges: [...rect(0, 0, 1, 1), ...ring([0.5, 0.5, 0], 0.05, 24)], more: [] };
  const c = snapAt({ x: 503, y: -504 }, face, flat);
  assert.equal(c.kind, 'centre'); nearP(c.p, [0.5, 0.5, 0], 1e-9);
  const rim = snapAt({ x: 551, y: -500 }, face, flat);
  assert.equal(rim.kind, 'edge');
});

test('a crossing with an edge of the face beside it is snapped to', () => {
  const face = { point: [0.5, 0.5, 0], edges: rect(0, 0, 1, 1), more: [[[0.3, -0.5, 0.5], [0.3, 0.5, 0.5]]] };
  const x = snapAt({ x: 303, y: 2 }, face, flat);
  assert.equal(x.kind, 'crossing');
  nearP(x.p, [0.3, 0, 0.5], 1e-9);
});

test('a point behind the eye is never snapped to', () => {
  const behind = p => ({ ...flat(p), behind: p[0] < 0.1 });
  const face = { point: [0.6, 0.4, 0], edges: rect(0, 0, 2, 1), more: [] };
  assert.notEqual(snapAt({ x: 2, y: -2 }, face, behind).kind, 'corner');
});

test('hostile input: no numbers, too many points, nothing at all, never throws and snaps to nothing it cannot see', () => {
  const bad = [[[NaN, 0, 0], [1, 0, 0]], [[0, Infinity, 0], [0, 1, 0]], [null, [0, 0, 0]], 'x', [[0, 0, 0]]];
  assert.doesNotThrow(() => loops(bad));
  assert.doesNotThrow(() => midpoints(bad));
  assert.doesNotThrow(() => corners(bad));
  assert.equal(circle([]), null);
  assert.equal(circle(null), null);
  assert.doesNotThrow(() => crossing(bad, { x: 0, y: 0 }, flat, 12));
  const huge = ring([0, 0, 0], 1, MAX_LOOP + 1);
  assert.equal(circle(loops(huge)[0] || []), null, 'a loop past MAX_LOOP is not searched');
  assert.equal(snapAt({ x: NaN, y: 0 }, { point: [0, 0, 0], edges: bad, more: bad }, flat)?.kind ?? 'face', 'face');
  assert.equal(snapAt({ x: 0, y: 0 }, null, flat), null);
});

test('each snap kind has its own shape, centred on the point', () => {
  const kinds = ['corner', 'midpoint', 'centre', 'crossing', 'edge', 'face'];
  const svgs = kinds.map(k => snapMark(k, 100, 50, 0, { ink: '#123456', halo: '#fff' }));
  assert.equal(new Set(svgs).size, kinds.length, 'six different shapes');
  for (const s of svgs) { assert.match(s, /#123456/); assert.doesNotMatch(s, /NaN|undefined/); }
  // An edge's bow tie lies flat along an edge nearer level and upright along one nearer plumb, never at a slant.
  assert.equal(snapMark('edge', 0, 0, 0.47, {}), snapMark('edge', 0, 0, 0, {}));
  assert.equal(snapMark('edge', 0, 0, 1.2, {}), snapMark('edge', 0, 0, Math.PI / 2, {}));
  assert.notEqual(snapMark('edge', 0, 0, 0, {}), snapMark('edge', 0, 0, Math.PI / 2, {}));
  assert.equal(snapMark('nothing', 0, 0, 0, {}), '');
  assert.doesNotMatch(snapMark('corner', NaN, 0, 0, {}), /NaN/);
});

test('the end styles: circle (today, the default) and a small cross; the choice is kept in the browser', () => {
  assert.deepEqual(END_STYLES, ['circle', 'cross']);
  const store = new Map(), ls = { getItem: k => (store.has(k) ? store.get(k) : null), setItem: (k, v) => store.set(k, String(v)) };
  assert.equal(endStyle(ls), 'circle');
  keepEnd(ls, 'cross');
  assert.equal(endStyle(ls), 'cross');
  keepEnd(ls, 'arrow');
  assert.equal(endStyle(ls), 'cross', 'a style not offered is not kept');
  store.set('bimcraft-viewer-measure-ends', 'garbage');
  assert.equal(endStyle(ls), 'circle');
  const broken = { getItem: () => { throw new Error('private window'); }, setItem: () => { throw new Error('private window'); } };
  assert.equal(endStyle(broken), 'circle');
  assert.doesNotThrow(() => keepEnd(broken, 'cross'));
});

test('an end is drawn as its style: a dot, or a cross turned 45 degrees to the line', () => {
  const ink = '#1c5ca3';
  assert.match(endMark('circle', 10, 20, 0, 1, { ink }), /<circle cx="10.0" cy="20.0" r="4.5"/);
  const x = endMark('cross', 10, 20, 0, 1, { ink, halo: '#fff' });
  assert.doesNotMatch(x, /circle/);
  // Along +y the cross's arms run at 45 degrees: from (7.2, 17.2) to (12.8, 22.8) and from (12.8, 17.2) to (7.2, 22.8).
  assert.match(x, /M7\.2 17\.2L12\.8 22\.8/);
  assert.match(x, /M12\.8 17\.2L7\.2 22\.8/);
  assert.equal(endMark('arrow', 0, 0, 1, 0, { ink }), endMark('circle', 0, 0, 1, 0, { ink }), 'an unknown style draws the default');
  assert.doesNotMatch(endMark('cross', 0, 0, NaN, 0, { ink }), /NaN/);
});

test('each end of a run of points is turned along its own line, outward', () => {
  const w = ways([{ x: 0, y: 0 }, { x: 10, y: 0 }]);
  nearP([w[0].x, w[0].y], [-1, 0]); nearP([w[1].x, w[1].y], [1, 0]);
  // An angle: its arms' ends outward from the corner, the corner along its first arm.
  const a = ways([{ x: 0, y: -10 }, { x: 0, y: 0 }, { x: 10, y: 0 }]);
  nearP([a[0].x, a[0].y], [0, -1]); nearP([a[1].x, a[1].y], [0, 1]); nearP([a[2].x, a[2].y], [1, 0]);
  // One point alone, two on top of each other, or no numbers: along x, never NaN.
  for (const pts of [[{ x: 5, y: 5 }], [{ x: 1, y: 1 }, { x: 1, y: 1 }], [{ x: NaN, y: 0 }, { x: 1, y: 0 }]]) {
    for (const u of ways(pts)) { assert.ok(Number.isFinite(u.x) && Number.isFinite(u.y)); near(Math.hypot(u.x, u.y), 1, 1e-12); }
  }
  assert.deepEqual(ways([]), []);
});

test('measure.js snaps through snaps.js and draws every snap shape and end mark from named colours, with no ball or plain dot of its own', () => {
  const src = readFileSync(new URL('../viewer/js/measure.js', import.meta.url), 'utf8');
  assert.match(src, /import \{[^}]*snapAt[^}]*\} from '\.\/snaps\.js'/);
  assert.match(src, /import \{[^}]*snapMark[^}]*\} from '\.\/snaps\.js'/);
  assert.match(src, /import \{[^}]*endMark[^}]*\} from '\.\/ends\.js'/);
  assert.doesNotMatch(src, /SphereGeometry/, 'the snap is a shape on the screen, not a ball in the scene');
  assert.doesNotMatch(src, /<circle/, 'ends are drawn by endMark');
  const calls = [...src.matchAll(/(snapMark|endMark)\(([^;]*?)\)[;,)]/g)];
  assert.ok(calls.length >= 2, 'snapMark and endMark are called');
  for (const [, , args] of calls) assert.doesNotMatch(args, /#[0-9a-f]{3,8}/i, 'colours come from named constants');
});
