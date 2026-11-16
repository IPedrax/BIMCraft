// How the walker's body moves, in plain numbers with no browser in it (metres, seconds, up positive). The numbers are what
// Autodesk's viewer was measured doing: nothing of it was read but the camera, frame by frame.
export const MAX_FALL = 10, ROLL_MOST = 3.2;
const GRAVITY = 9.8, RISE = 1.5, NOTCH = 2.1, GLIDE = 5;

// One frame of gravity. `floor` is the floor under the feet, null where there is none: then the height is held, so a walker
// over nothing flies on. A floor at the feet or just over them (a step) is stood on at once; one below is fallen to, the speed
// building up to MAX_FALL, which makes a long way down a slow descent.
export function fall(y, floor, vy, dt) {
  if (floor === null) return { y, vy: 0 };
  if (floor >= y - 0.01) return { y: floor, vy: 0 };
  vy = Math.max(vy - GRAVITY * dt, -MAX_FALL);
  y = Math.max(floor, y + vy * dt);
  return { y, vy: y <= floor ? 0 : vy };
}
// The floor an eye must stand up on when Walk starts: the one under it, if that is nearer than eye height. Else gravity, which
// looks from a step over the feet, would take a low eye for one under the floor and drop it through.
export const liftTo = (eye, floor, height) => floor !== null && eye - floor < height ? floor : null;
// One frame of standing up, toward `to`.
export const rise = (y, to, dt) => Math.min(to, y + RISE * dt);

// How far is left to roll after one wheel event: forward when rolled away. A small event (a touch pad's, a Mac wheel's) counts
// a quarter notch at least, and the rolls wait in a short queue, so a long roll goes on at a steady speed.
export function roll(left, e) {
  const d = e.deltaMode === 1 ? e.deltaY * 33 : e.deltaY, notches = Math.min(1, Math.max(0.25, Math.abs(d) / 100));
  return Math.min(ROLL_MOST, Math.max(-ROLL_MOST, left - Math.sign(d) * notches * NOTCH));
}
// One frame of a roll: most of the way in the first half second, and the last of it done with rather than left to dwindle.
export function glide(left, dt) {
  const step = left * (1 - Math.exp(-GLIDE * dt));
  return Math.abs(left - step) < 0.005 ? { step: left, left: 0 } : { step, left: left - step };
}

// Walking straight. A heading (radians, as the walker's yaw: 0 faces −z) within `most` of square to a wall is put square to it;
// `walls` are the level ways the faces round the walker point (atan2(x, z) of each face's normal), and a wall is square to a
// heading along it or across it. Else the heading is left as it is.
export function square(yaw, walls, most = 4 * Math.PI / 180) {
  let best = yaw, off = most;
  for (const w of walls) {
    for (let k = 0; k < 4; k++) {
      const h = w + k * Math.PI / 2, d = Math.atan2(Math.sin(h - yaw), Math.cos(h - yaw));
      if (Math.abs(d) < off) { off = Math.abs(d); best = yaw + d; }
    }
  }
  return best;
}
// How far a body may go toward a face before it is `radius` from it, square to the face: `gap` is how far the body's middle is
// from the face's plane now, `cos` how square the way is to the face (1 straight at it, 0 along it). Measured along the walk, as it
// once was, the gap shrank to nothing when a wall was met aslant, and the eye came through it.
export const gapRoom = (gap, cos, radius) => (gap - radius) / Math.max(cos, 0.05);
