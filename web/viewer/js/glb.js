// What a glTF file names outside itself. A buffer or a picture at another address would make the browser fetch it when
// the file is opened (a link to this page can name any file), so the viewer refuses such a file before reading it.
// BIMCraft's .glb keeps everything in its own binary chunk, and a picture written as data: is inside the file too.

const GLTF = 0x46546c67, JSON_CHUNK = 0x4e4f534a;

function json(buffer) {
  const bytes = new Uint8Array(buffer), v = new DataView(buffer);
  let text;
  if (bytes.length >= 20 && v.getUint32(0, true) === GLTF) {
    const length = v.getUint32(12, true);
    if (v.getUint32(16, true) !== JSON_CHUNK || 20 + length > bytes.length) return null;
    text = new TextDecoder().decode(bytes.subarray(20, 20 + length));
  } else {
    text = new TextDecoder().decode(bytes);
  }
  try { return JSON.parse(text); } catch { return null; }
}

// Every buffer's and picture's `uri` that is not data: (an empty list for a file that names nothing, or isn't glTF).
export function outsideAddresses(buffer) {
  const doc = json(buffer);
  if (!doc || typeof doc !== 'object') return [];
  const list = k => (Array.isArray(doc[k]) ? doc[k] : []);
  return [...list('buffers'), ...list('images')]
    .map(x => x?.uri)
    .filter(uri => typeof uri === 'string' && !/^data:/i.test(uri));
}
