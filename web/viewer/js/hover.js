// When Measure finds again what is under the pointer, in plain numbers with no browser in it, so `web/tests/faces.test.mjs` runs
// it. Once a frame at most, at the pointer's last place: several moves in one frame are one snap. While a press moves the view
// (an orbit, a pan) nothing is shown under the pointer, since what was under it has moved away; when it lets go, and whenever the
// view moves under a still pointer (the wheel, a flight), the snap is found again where the pointer is.
export class Hover {
  constructor() { this.at = null; this.down = false; this.due = false; }
  // The pointer moved with nothing pressed.
  move(x, y) { this.at = { x, y }; if (!this.down) this.due = true; }
  press() { this.down = true; this.due = false; }
  release(x, y) { this.down = false; this.at = { x, y }; this.due = true; }
  // The view moved.
  view() { if (this.at && !this.down) this.due = true; }
  // The pointer left the picture.
  leave() { this.at = null; this.due = false; }
  // Whether anything may be shown under the pointer.
  get shown() { return !this.down && !!this.at; }
  // Where to snap this frame, or null.
  take() { if (!this.due || !this.shown) return null; this.due = false; return this.at; }
}
