// The ViewCube: a cube of our own drawing with its edges and corners cut flat, so its 26 views show before the pointer
// finds them. A click on a part turns the view to look from there, and from the far side when it already looks at that
// face; a drag orbits, on the ring only about the vertical; N, E, S and W turn it level; arrows step round the faces; at the Top and the Bottom two buttons
// turn the picture a quarter. Drawn flat on a 2D canvas, so it asks nothing of the graphics card.
// In the model's terms, as the desktop: x east, y north, z up. nav.js turns them into the picture's.

export const CUBE = { cut: 0.2, ring: 1.9375, band: 0.5, letters: 2.3, turn: 0.45, keys: [15, 90, 5], drag: 0.4, rest: 0.62, home: [-45, 30], sizes: [[124, 24], [96, 18.5], [64, 13]], views: [[700, 420], [360, 260]], words: [[[0, 0, 1], [1, 0, 0], [0, 1, 0]], [[0, 0, -1], [1, 0, 0], [0, -1, 0]], [[0, -1, 0], [1, 0, 0], [0, 0, 1]], [[0, 1, 0], [-1, 0, 0], [0, 0, 1]], [[-1, 0, 0], [0, -1, 0], [0, 0, 1]], [[1, 0, 0], [0, 1, 0], [0, 0, 1]]] };

const RAD = Math.PI / 180, REACH = 1 - CUBE.cut, CORNER = 1 - 2 * CUBE.cut, RING_Z = -1;
const NAMES = { '0,0,1': 'Top', '0,0,-1': 'Bottom', '0,-1,0': 'Front', '0,1,0': 'Back', '-1,0,0': 'Left', '1,0,0': 'Right' };
const POINTS = [['N', 0, 1], ['E', 1, 0], ['S', 0, -1], ['W', -1, 0]];
const COMPASS = { N: 'north', E: 'east', S: 'south', W: 'west' };
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const same = (a, b) => a && b && a[0] === b[0] && a[1] === b[1] && a[2] === b[2];
const wrapped = a => { const r = ((a % 360) + 360) % 360; return r > 180 ? r - 360 : r; };
// Rounding as Rust's: away from zero at a half.
const round = v => Math.sign(v) * Math.round(Math.abs(v));

// Toward the camera, the picture's right and its up, as `Camera::axes`.
function axes(az, el) {
  const a = az * RAD, e = el * RAD, c = Math.cos(e), s = Math.sin(e);
  return { back: [c * Math.cos(a), c * Math.sin(a), s], right: [-Math.sin(a), Math.cos(a), 0], up: [-s * Math.cos(a), -s * Math.sin(a), c] };
}
export function facing(spot, az) {
  const [x, y, z] = spot;
  if (!x && !y) { const q = wrapped(round(az / 90) * 90); return [q, 90 * z]; }
  return [Math.atan2(y, x) / RAD, Math.atan2(z, Math.hypot(x, y)) / RAD];
}
function parts() {
  const out = [], at = (k, i, j, pk, pi, pj) => { const p = [0, 0, 0]; p[k] = pk; p[i] = pi; p[j] = pj; return p; };
  for (let k = 0; k < 3; k++) for (const s of [1, -1]) {
    const i = (k + 1) % 3, j = (k + 2) % 3, spot = [0, 0, 0]; spot[k] = s;
    out.push({ spot, kind: 'face', poly: [[CORNER, -REACH], [REACH, -CORNER], [REACH, CORNER], [CORNER, REACH], [-CORNER, REACH], [-REACH, CORNER], [-REACH, -CORNER], [-CORNER, -REACH]].map(([u, v]) => at(k, i, j, s, u, v)) });
  }
  for (let k = 0; k < 3; k++) {
    const i = (k + 1) % 3, j = (k + 2) % 3;
    for (const si of [1, -1]) for (const sj of [1, -1]) {
      const spot = [0, 0, 0]; spot[i] = si; spot[j] = sj;
      out.push({ spot, kind: 'edge', poly: [at(k, i, j, CORNER, si, sj * REACH), at(k, i, j, CORNER, si * REACH, sj), at(k, i, j, -CORNER, si * REACH, sj), at(k, i, j, -CORNER, si, sj * REACH)] });
    }
  }
  for (const x of [1, -1]) for (const y of [1, -1]) for (const z of [1, -1])
    out.push({ spot: [x, y, z], kind: 'corner', poly: [[REACH, CORNER, 1], [CORNER, REACH, 1], [CORNER, 1, REACH], [REACH, 1, CORNER], [1, REACH, CORNER], [1, CORNER, REACH]].map(([a, b, c]) => [a * x, b * y, c * z]) });
  return out.map(t => { const l = Math.hypot(...t.spot); return { ...t, n: t.spot.map(v => v / l) }; });
}
const PARTS = parts();

export function lookingFrom(az, el) {
  const off = a => Math.abs(wrapped(a - az));
  return PARTS.find(t => { const [a, e] = facing(t.spot, az); return Math.abs(e - el) < 0.3 && (Math.abs(e) > 89.9 || off(a) < 0.3); }) || null;
}
const inside = (pts, x, y) => {
  let s = 0;
  for (let i = 0; i < pts.length; i++) {
    const a = pts[i], b = pts[(i + 1) % pts.length], c = (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
    if (c) { if (s && Math.sign(c) !== s) return false; s = Math.sign(c); }
  }
  return s !== 0;
};
function offLine(p, a, b) {
  const dx = b[0] - a[0], dy = b[1] - a[1], t = Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy || 1e-12)));
  return Math.hypot(p[0] - a[0] - t * dx, p[1] - a[1] - t * dy);
}
const offRing = (p, pts) => Math.min(...pts.map((a, i) => offLine(p, a, pts[(i + 1) % pts.length])));

// The cube as it stands: `S` is {az, el, size index, touch, active}; all points in px from the top left of its square.
function layout(S) {
  const [z, half] = CUBE.sizes[S.tier], ax = axes(S.az, S.el), mid = z / 2;
  const pr = p => [mid + dot(p, ax.right) * half, mid - dot(p, ax.up) * half];
  const shown = PARTS.map(t => ({ ...t, toward: dot(t.n, ax.back) })).filter(t => t.toward > 1e-6).map(t => ({ ...t, pts: t.poly.map(pr) }));
  const ring = []; for (let k = 0; k < 64; k++) { const a = k * Math.PI * 2 / 64; ring.push(pr([CUBE.ring * Math.cos(a), CUBE.ring * Math.sin(a), RING_Z])); }
  const here = lookingFrom(S.az, S.el), working = S.active || S.touch;
  const letters = POINTS.filter(([l]) => S.tier === 0 || (S.tier === 1 && l === 'N')).map(([l, x, y]) => {
    const at = pr([x * CUBE.letters, y * CUBE.letters, RING_Z]);
    return { l, x, y, at, far: dot([x, y, 0], ax.back) < -0.01 };
  }).filter(p => !(p.far && shown.some(t => inside(t.pts, p.at[0], p.at[1]))));
  const b = S.tier === 2 ? (S.touch ? 20 : 16) : S.touch ? 26 : 22, near = 2, far = z - b - 2, next = near + b + 2, controls = [];
  if (working) {
    controls.push({ id: 'menu', x: far, y: far, w: b });
    if (S.tier !== 2) controls.push({ id: 'home', x: near, y: near, w: b });
    if (S.tier === 0) controls.push({ id: 'proj', x: next, y: near, w: b });
    if (S.tier !== 2 && here && !here.spot[0] && !here.spot[1]) controls.push({ id: 'rollL', x: near, y: far, w: b }, { id: 'rollR', x: next, y: far, w: b });
  }
  const steps = working && S.tier !== 2 && here?.kind === 'face'
    ? [[0, -1], [1, 0], [0, 1], [-1, 0]].map(([dx, dy]) => ({ dx, dy, x: mid + dx * 1.42 * half, y: mid + dy * 1.42 * half })) : [];
  return { z, half, band: half * CUBE.band, ax, pr, shown, ring, here, letters, controls, steps };
}
function hit(S, L, x, y) {
  const [slop, reach, arrow] = S.touch ? [8, 12, 10] : [2, 9, 7], away = (px, py) => Math.hypot(px - x, py - y);
  const c = L.controls.find(c => x >= c.x - slop && x <= c.x + c.w + slop && y >= c.y - slop && y <= c.y + c.w + slop);
  const l = L.letters.find(l => away(...l.at) < reach);
  if (c && !(l && away(...l.at) < away(c.x + c.w / 2, c.y + c.w / 2))) return { type: 'btn', id: c.id };
  if (l) return { type: 'letter', l };
  const n = L.steps.find(n => away(n.x, n.y) < arrow);
  if (n) return { type: 'step', n };
  const t = L.shown.find(t => inside(t.pts, x, y));
  if (t) return { type: 'part', spot: t.spot, kind: t.kind };
  if (S.touch) {
    let best = null, bd = 10;
    for (const t of L.shown) { const d = offRing([x, y], t.pts); if (d < bd) { bd = d; best = t; } }
    if (best) return { type: 'part', spot: best.spot, kind: best.kind };
  }
  return offRing([x, y], L.ring) <= L.band / 2 + 2 ? { type: 'ring' } : null;
}
function sized(w, h) {
  const within = ([vw, vh]) => w <= vw || h <= vh;
  return within(CUBE.views[1]) ? 2 : within(CUBE.views[0]) ? 1 : 0;
}
const poly = (g, pts) => { g.beginPath(); pts.forEach((p, i) => i ? g.lineTo(p[0], p[1]) : g.moveTo(p[0], p[1])); g.closePath(); };
const FONT = '"IBM Plex Sans", system-ui, sans-serif';

export class Cube {
  constructor(app, nav) {
    this.app = app; this.nav = nav;
    const canvas = this.canvas = document.getElementById('cube');
    this.g = canvas.getContext('2d');
    this.S = { az: 0, el: 0, tier: 0, touch: matchMedia('(pointer: coarse)').matches, active: false };
    this.hot = null; this.pressed = false; this.size = 0;
    this.readTheme(); matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => { this.readTheme(); this.draw(); });
    let press = null;
    const at = e => { const r = canvas.getBoundingClientRect(); return [e.clientX - r.left, e.clientY - r.top]; };
    const look = (S = this.S) => layout(S);
    canvas.addEventListener('pointerdown', e => {
      if (e.pointerType === 'touch') this.S.touch = true;
      this.app.closeMenu?.();
      if (e.button !== 0) return;
      canvas.setPointerCapture(e.pointerId);
      const [x, y] = at(e), h = hit(this.S, look(), x, y);
      press = { x, y, lx: x, ly: y, moved: false, h }; nav.fly = null; this.hot = h; this.pressed = !!h; this.draw();
    });
    canvas.addEventListener('pointermove', e => {
      const [x, y] = at(e);
      if (!press) { this.S.active = true; this.hot = hit(this.S, look(), x, y); this.tip(); return this.draw(); }
      if (Math.hypot(x - press.x, y - press.y) > 3) press.moved = true;
      if (press.moved) {
        const spin = press.h?.type === 'ring' || press.h?.type === 'letter', k = CUBE.drag * RAD / 0.006;
        nav.orbit((x - press.lx) * k, spin ? 0 : (y - press.ly) * k, nav.centre()); press.lx = x; press.ly = y;
        if (!spin) this.hot = null;
        this.pressed = false;
      }
    });
    canvas.addEventListener('pointerup', () => {
      const p = press; press = null; this.pressed = false;
      if (p && !p.moved) this.act(p.h, p.x, p.y);
      this.draw();
    });
    canvas.addEventListener('pointercancel', () => { press = null; this.pressed = false; this.draw(); });
    canvas.addEventListener('pointerleave', () => { if (press) return; this.S.active = document.activeElement === canvas; this.hot = null; this.draw(); });
    canvas.addEventListener('contextmenu', e => { e.preventDefault(); const [x, y] = at(e); this.menu(x, y); });
    canvas.addEventListener('focus', () => { this.S.active = true; this.draw(); });
    canvas.addEventListener('blur', () => { this.S.active = false; this.draw(); });
  }
  // The colours of the page, read again as its theme turns.
  readTheme() {
    const css = getComputedStyle(document.documentElement), v = n => css.getPropertyValue(n).trim();
    const dark = matchMedia('(prefers-color-scheme: dark)').matches;
    this.pal = dark
      ? { accent: v('--accent'), ink: v('--fg'), muted: v('--muted'), line: v('--line'), surface: '#222c38', edge: '#2c3846', corner: '#364454', ground: v('--bg'), onAccent: v('--accent-fg'), x: '#f0716a', y: '#4fc47f', z: v('--accent') }
      : { accent: v('--accent'), ink: v('--fg'), muted: v('--muted'), line: '#c4ccd6', surface: '#ffffff', edge: '#e8ecf1', corner: '#dbe1e9', ground: v('--bg'), onAccent: v('--accent-fg'), x: '#d2453d', y: '#2e9e5b', z: v('--accent') };
  }
  tip() {
    const h = this.hot, words = { home: 'Home view (H)', proj: this.app.parallel ? 'Turn to perspective' : 'Turn to parallel', rollL: 'Turn the picture left', rollR: 'Turn the picture right', menu: 'View menu' };
    this.canvas.title = h?.type === 'btn' ? words[h.id] : h?.type === 'letter' ? `Look from the ${COMPASS[h.l.l]}` : h?.type === 'ring' ? 'Drag to turn level' : h?.type === 'step' ? 'Go to the next face' : 'Click a face, edge or corner; drag to turn';
    this.canvas.style.cursor = h ? 'pointer' : 'grab';
  }
  // What a click on `h` does.
  act(h, x, y) {
    if (!h) return;
    const { az, el } = this.S, nav = this.nav;
    if (h.type === 'part') {
      const again = h.kind === 'face' && same(lookingFrom(az, el)?.spot, h.spot);
      nav.turn(...facing(again ? h.spot.map(v => -v) : h.spot, az));
    } else if (h.type === 'letter') nav.turn(Math.atan2(h.l.y, h.l.x) / RAD, el);
    else if (h.type === 'step') {
      const a = axes(az, el), spot = [0, 1, 2].map(i => Math.round(h.n.dx * a.right[i] - h.n.dy * a.up[i]) + 0);
      nav.turn(...facing(spot, az));
    } else if (h.type === 'btn') {
      if (h.id === 'home') nav.home();
      if (h.id === 'proj') this.project(!this.app.parallel);
      if (h.id === 'menu') this.menu(x, y);
      if (h.id === 'rollL' || h.id === 'rollR') {
        const from = lookingFrom(az, el), clockwise = h.id === 'rollR', turn = clockwise === (from?.spot[2] > 0) ? 90 : -90;
        nav.turn(wrapped(az + turn), el);
      }
    }
  }
  // The cube's menu, where the model's menu opens. This page only looks, so it has no Set as Home.
  menu(x, y) {
    const r = this.canvas.getBoundingClientRect();
    this.app.openMenuAt?.(r.left + x, r.top + y, [['Home view', () => this.nav.home()], null,
      ['Parallel', () => this.project(true), this.app.parallel], ['Perspective', () => this.project(false), !this.app.parallel]]);
  }
  // Parallel or perspective. Walk is in perspective, so turning to parallel ends it.
  project(parallel) { if (parallel) this.app.leaveWalk?.(); this.app.setParallel(parallel); this.tip(); }
  // An arrow key, or Home: true if it was one of those.
  key(e) {
    if (e.key === 'Home') { e.preventDefault(); this.nav.home(); return true; }
    const d = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, 1], ArrowDown: [0, -1] }[e.key];
    if (!d) return false;
    e.preventDefault();
    const step = CUBE.keys[e.shiftKey ? 1 : e.ctrlKey || e.metaKey ? 2 : 0], [az, el] = this.nav.heading();
    this.nav.turn(wrapped(az + d[0] * step), Math.max(-90, Math.min(90, el + d[1] * step)));
    return true;
  }
  draw() {
    const v = this.app.size?.(), S = this.S, [az, el] = this.nav.angles();
    if (!(v?.w > 0)) return;
    S.az = az; S.el = el;
    const tier = sized(v.w, v.h);
    if (tier !== S.tier || !this.size) {
      S.tier = tier; this.size = CUBE.sizes[tier][0];
      this.canvas.style.width = this.canvas.style.height = this.size + 'px';
    }
    const k = Math.min(window.devicePixelRatio || 1, 3), px = Math.round(this.size * k), c = this.canvas, g = this.g;
    if (c.width !== px) { c.width = c.height = px; }
    g.setTransform(k, 0, 0, k, 0, 0); g.clearRect(0, 0, this.size, this.size);
    this.paint(g, layout(S));
  }
  paint(g, L) {
    const S = this.S, p = this.pal, hot = this.hot, awake = S.active || S.touch, rest = awake ? 1 : CUBE.rest;
    g.globalAlpha = rest;
    const hotRing = hot?.type === 'ring', ring = () => {
      poly(g, L.ring); g.strokeStyle = hotRing ? p.accent : p.line; g.lineWidth = hotRing ? L.band + 1 : L.band; g.stroke();
      poly(g, L.ring); g.strokeStyle = mix(p.surface, p.line, 0.6); g.lineWidth = hotRing ? L.band - 1 : L.band - 2; g.stroke();
      if (S.tier !== 2) for (let k = 0; k < 8; k++) {
        const a = k * Math.PI / 4, r = CUBE.ring + CUBE.band / 2, out = r - CUBE.band * (k % 2 ? 0.3 : 0.55), o = L.pr([r * Math.cos(a), r * Math.sin(a), RING_Z]), e = L.pr([out * Math.cos(a), out * Math.sin(a), RING_Z]);
        g.beginPath(); g.moveTo(...o); g.lineTo(...e); g.strokeStyle = p.muted; g.lineWidth = 1; g.stroke();
      }
    };
    if (S.el >= 0) ring();
    const order = { face: 0, edge: 1, corner: 2 };
    for (const t of L.shown.slice().sort((a, b) => order[a.kind] - order[b.kind])) {
      const isHot = hot?.type === 'part' && same(hot.spot, t.spot), isHere = same(L.here?.spot, t.spot);
      poly(g, t.pts);
      g.fillStyle = t.kind === 'face' ? mix(p.surface, p.line, (1 - t.toward) * 0.42) : t.kind === 'edge' ? mix(p.edge, p.line, (1 - t.toward) * 0.3) : p.corner; g.fill();
      if (isHere && !isHot) { g.fillStyle = alpha(p.accent, 0.14); g.fill(); }
      if (isHot) { g.fillStyle = this.pressed ? p.accent : alpha(p.accent, t.kind === 'face' ? 0.26 : 0.6); g.fill(); }
      g.strokeStyle = isHot ? p.accent : p.line; g.lineWidth = 1; g.lineJoin = 'round'; g.stroke();
    }
    g.textAlign = 'center'; g.textBaseline = 'middle';
    const tiny = S.tier === 2, fs = tiny ? 11 : S.tier === 1 ? 9.5 : 12;
    g.font = `600 ${fs}px ${FONT}`;
    for (const t of L.shown) {
      if (t.kind !== 'face' || t.toward <= 0.15) continue;
      const entry = CUBE.words.find(w => same(w[0], t.spot)), word = NAMES[t.spot.join(',')], [r, u] = [entry[1], entry[2]];
      const a = [dot(r, L.ax.right), -dot(r, L.ax.up)], b = [dot(u, L.ax.right), -dot(u, L.ax.up)], m = L.pr(t.spot);
      const pressed = this.pressed && hot?.type === 'part' && same(hot.spot, t.spot), text = tiny ? word[0] : word;
      g.save(); g.transform(a[0], a[1], -b[0], -b[1], m[0], m[1]);
      g.fillStyle = pressed ? p.onAccent : same(L.here?.spot, t.spot) ? p.accent : p.ink;
      g.globalAlpha = rest * (0.5 + 0.5 * Math.min(1, t.toward * 1.4));
      g.fillText(text, 0, 0, L.half * 1.7); g.restore();
    }
    g.globalAlpha = rest;
    for (const n of L.steps) {
      const on = hot?.type === 'step' && hot.n.dx === n.dx && hot.n.dy === n.dy, s = on ? 6 : 5, px = -n.dy, py = n.dx;
      g.beginPath(); g.moveTo(n.x + n.dx * s, n.y + n.dy * s); g.lineTo(n.x - n.dx * 2 + px * s, n.y - n.dy * 2 + py * s); g.lineTo(n.x - n.dx * 2 - px * s, n.y - n.dy * 2 - py * s); g.closePath();
      g.fillStyle = on ? p.accent : p.muted; g.fill();
    }
    if (S.tier === 0) {
      const o = [L.z - 22, 22], len = 13;
      g.lineCap = 'round';
      const axis = [[[1, 0, 0], p.x, 'X'], [[0, 1, 0], p.y, 'Y'], [[0, 0, 1], p.z, 'Z']].sort((a, b) => dot(a[0], L.ax.back) - dot(b[0], L.ax.back));
      for (const [v, c, t] of axis) {
        const ex = dot(v, L.ax.right), ey = -dot(v, L.ax.up);
        g.strokeStyle = c; g.lineWidth = 2; g.beginPath(); g.moveTo(...o); g.lineTo(o[0] + ex * len, o[1] + ey * len); g.stroke();
        g.font = `600 9px ${FONT}`; g.lineWidth = 2.5; g.strokeStyle = p.ground; g.strokeText(t, o[0] + ex * (len + 6), o[1] + ey * (len + 6));
        g.fillStyle = c; g.fillText(t, o[0] + ex * (len + 6), o[1] + ey * (len + 6));
      }
      g.lineCap = 'butt';
    }
    for (const c of L.controls) this.button(g, c, hot?.type === 'btn' && hot.id === c.id);
    if (S.el < 0) ring();
    for (const l of L.letters) {
      const on = hot?.type === 'letter' && hot.l.l === l.l, N = l.l === 'N';
      g.font = `${N ? 700 : 600} 11px ${FONT}`;
      if (on) { g.fillStyle = p.accent; g.beginPath(); g.arc(l.at[0], l.at[1], 8.5, 0, 7); g.fill(); g.fillStyle = p.onAccent; }
      else { g.lineWidth = 3; g.strokeStyle = p.ground; g.lineJoin = 'round'; g.strokeText(l.l, ...l.at); g.fillStyle = N ? p.accent : p.muted; }
      g.fillText(l.l, ...l.at);
    }
    g.globalAlpha = 1;
  }
  button(g, c, on) {
    const p = this.pal, { x, y, w, id } = c, mx = x + w / 2, my = y + w / 2;
    if (id === 'menu') {
      if (on) { g.fillStyle = p.accent; g.beginPath(); g.arc(mx, my, 8.5, 0, 7); g.fill(); }
      const mark = () => { g.beginPath(); g.moveTo(mx - 3.5, my - 1.7); g.lineTo(mx, my + 1.8); g.lineTo(mx + 3.5, my - 1.7); g.stroke(); };
      g.lineJoin = g.lineCap = 'round';
      if (!on) { g.strokeStyle = p.ground; g.lineWidth = 4; mark(); }
      g.strokeStyle = on ? p.onAccent : p.muted; g.lineWidth = 1.5; mark(); g.lineCap = 'butt';
      return;
    }
    g.fillStyle = on ? p.accent : p.surface; g.strokeStyle = on ? p.accent : p.line; g.lineWidth = 1;
    g.beginPath(); g.rect(x + 0.5, y + 0.5, w - 1, w - 1); g.fill(); g.stroke();
    g.strokeStyle = on ? p.onAccent : p.ink; g.lineWidth = 1.4; g.lineJoin = g.lineCap = 'round'; g.beginPath();
    if (id === 'home') { g.moveTo(mx - 6, my); g.lineTo(mx, my - 5.5); g.lineTo(mx + 6, my); g.moveTo(mx - 4.5, my - 1); g.lineTo(mx - 4.5, my + 5.5); g.lineTo(mx + 4.5, my + 5.5); g.lineTo(mx + 4.5, my - 1); g.stroke(); }
    if (id === 'proj') {
      if (this.app.parallel) g.rect(mx - 5, my - 5, 10, 10);
      else { g.moveTo(mx - 3.5, my - 5); g.lineTo(mx + 3.5, my - 5); g.lineTo(mx + 6, my + 5); g.lineTo(mx - 6, my + 5); g.closePath(); }
      g.stroke();
    }
    if (id === 'rollL' || id === 'rollR') {
      const f = id === 'rollR' ? 1 : -1, end = f > 0 ? 1 : 2.14; g.arc(mx, my, 5.5, f > 0 ? -2.6 : -0.54, end, f < 0); g.stroke();
      const hx = mx + 5.5 * Math.cos(end), hy = my + 5.5 * Math.sin(end); g.beginPath(); g.moveTo(hx - f * 3.4, hy + 0.4); g.lineTo(hx, hy); g.lineTo(hx + f * 0.6, hy - 3.4); g.stroke();
    }
    g.lineCap = 'butt';
  }
}
function rgb(c) {
  if (c.startsWith('#')) return [1, 3, 5].map(i => parseInt(c.substr(i, 2), 16));
  return c.match(/[\d.]+/g).slice(0, 3).map(Number);
}
const mix = (a, b, t) => { const p = rgb(a), q = rgb(b); return `rgb(${p.map((v, i) => Math.round(v + (q[i] - v) * t)).join(',')})`; };
const alpha = (c, a) => `rgba(${rgb(c).join(',')},${a})`;
