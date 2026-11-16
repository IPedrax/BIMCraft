// The cursor says what a drag will do, as desktop viewers show it: orbit, pan and zoom; Walk has a gun sight whatever is
// held, as Autodesk's First Person does, a plus with a gap at its middle so what it points at stays seen. Each is drawn here,
// a dark line over a white one so it reads on any colour, and handed to CSS as a picture with a plain cursor behind it.
const PATHS = {
  orbit: 'M19 12a7 7 0 1 1-2.05-4.95M19 4v4h-4',
  pan: 'M12 3v18M3 12h18M12 3l-3 3M12 3l3 3M12 21l-3-3M12 21l3-3M3 12l3-3M3 12l3 3M21 12l-3-3M21 12l-3 3',
  zoom: 'M4.5 10.5a6 6 0 1 0 12 0a6 6 0 1 0-12 0M15 15l5.5 5.5M8 10.5h5M10.5 8v5',
  walk: 'M12 1v8M12 15v8M1 12h8M15 12h8',
};
const PLAIN = { orbit: 'grab', pan: 'move', zoom: 'zoom-in', walk: 'crosshair' };

export function makeCursors() {
  const out = {};
  for (const [k, d] of Object.entries(PATHS)) {
    const c = document.createElement('canvas'); c.width = c.height = 28;
    const g = c.getContext('2d'); g.translate(2, 2); g.lineCap = g.lineJoin = 'round';
    const p = new Path2D(d);
    g.strokeStyle = '#fff'; g.lineWidth = 4.5; g.stroke(p);
    g.strokeStyle = '#17202b'; g.lineWidth = 1.8; g.stroke(p);
    out[k] = `url(${c.toDataURL()}) 14 14, ${PLAIN[k]}`;
  }
  return out;
}
