// The light of the 3D view, worked out from the desktop's (the core's `lighting`, kept the same in gpu.wgsl) so a face comes out
// as bright here as there: a wall out of the sun at 0.55 of its colour as shown, a roof at 0.65, a ceiling at 0.9, and a sunlit
// roof at all of it. three.js adds light in linear terms and divides by pi; the desktop multiplies the colour as it is shown,
// so each number here is one of the desktop's in linear terms. The desktop's light follows the eye and the sun here stands
// still, for its shadows, so a sunlit wall can differ by the way it faces.
const linear = v => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
const WALL = linear(0.55), ROOF = linear(0.65), CEILING = linear(0.9);
// Where the sun stands, as three.js's y up.
export const SUN = [1, 2, 1.5];
const high = SUN[1] / Math.hypot(...SUN);
const ground = 2 * WALL / ROOF - 1;
export const LIGHT = {
  sky: ROOF * Math.PI, // up top: the sky alone
  ground, // what the hemisphere's lower half is of the sky, linear: the two together light a wall
  sun: (1 - ROOF) * Math.PI / high, // what a roof lacks, in the sun
  bounce: (CEILING - ground * ROOF) * Math.PI, // what a ceiling lacks: the floor throws it back, onto what faces down alone
};
