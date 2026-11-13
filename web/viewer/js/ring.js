// The loader: one ring in the middle of the view, with how far the model has come in it, and no words. What is counted is real
// work: the bytes received and the elements placed; reading the file is the one step that cannot be counted, and the ring holds
// there.

// Each step's share, in order, from the start, as the time went on a 16 MB model fetched over a local network: receiving 2.9 s,
// reading 0.15 s, placing 0.3 s.
export const SPANS = { receiving: [0, 0.85], reading: [0.85, 0.9], placing: [0.9, 1] };
// How long a load may take before the ring shows: a quick one shows nothing.
export const DELAY = 400;

// How far the whole load is, from 0 to 1, when `step` is `f` (0 to 1) done.
export function overall(step, f) {
  const span = SPANS[step];
  if (!span || !Number.isFinite(f)) return null;
  const k = Math.min(1, Math.max(0, f));
  return span[0] + (span[1] - span[0]) * k;
}

// The ring on the page: `el` holds an arc (`.arc`, a circle of radius `r`) and the number (`.pct`). It shows `DELAY` after
// `start`, spins until a step says how far it is, never goes back, and goes at `stop`.
export class Ring {
  constructor(el, r = 26, timers = { set: (f, ms) => setTimeout(f, ms), clear: id => clearTimeout(id) }) {
    Object.assign(this, { el, timers, round: 2 * Math.PI * r, f: 0, on: false, timer: null });
    this.arc = el.querySelector('.arc'); this.pct = el.querySelector('.pct');
    if (this.arc) this.arc.style.strokeDasharray = String(this.round);
  }

  start() {
    this.stop();
    Object.assign(this, { f: 0, on: true, counted: false });
    this.draw();
    this.timer = this.timers.set(() => { if (this.on) this.el.hidden = false; }, DELAY);
  }

  // `step` is `f` done.
  at(step, f) {
    if (!this.on) return;
    const g = overall(step, f);
    if (g === null) return;
    this.counted = true;
    this.f = Math.max(this.f, g);
    this.draw();
  }

  draw() {
    this.el.classList.toggle('spin', !this.counted);
    if (this.arc) this.arc.style.strokeDashoffset = String(this.round * (1 - (this.counted ? this.f : 0.25)));
    // 100 only once it is shown: the ring goes as the model does.
    if (this.pct) this.pct.textContent = this.counted ? Math.min(99, Math.floor(this.f * 100)) + '%' : '';
  }

  stop() {
    this.on = false;
    if (this.timer !== null) this.timers.clear(this.timer);
    this.timer = null; this.el.hidden = true;
  }
}
