// Follow: a model named in the address (?file=) is fetched again when the file changes, and the status dot says so.

// The status dot: its state, and the words it holds for a hover and a screen reader.
export function show(chip, state, text) { chip.hidden = false; chip.dataset.state = state; chip.dataset.tip = text; chip.ariaLabel = text; }
// A fetch of the whole file, on the dot.
export async function updating(chip, reload) {
  show(chip, 'wait', 'Updating');
  try { await reload(); show(chip, 'live', 'Updated ' + new Date().toLocaleTimeString()); }
  catch { show(chip, 'off', 'Could not fetch the model'); }
}

// A model named in the address (?file=): the server that gave it says when it changed, so the page asks it and fetches the file
// again when it did, the camera and the selection staying. What it asks is only ever that address. Nothing is asked while the page
// is hidden, and a failed look is passed over.
export function followFile(url, reload, chip, every = 2000) {
  const mark = r => [r.headers.get('etag'), r.headers.get('last-modified'), r.headers.get('content-length')].join('|');
  let seen = null, busy = false;
  const look = async () => {
    if (busy || document.hidden) return;
    busy = true;
    try {
      const r = await fetch(url, { method: 'HEAD', cache: 'no-store' });
      const now = r.ok ? mark(r) : seen;
      if (seen === null) seen = now;
      else if (now !== seen) { seen = now; await updating(chip, reload); }
    } catch { /* the next look may do */ } finally { busy = false; }
  };
  look();
  setInterval(look, every);
}
