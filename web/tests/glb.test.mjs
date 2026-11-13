// A file names nothing outside itself: a buffer or picture at another address would make the browser fetch it when the
// file is opened, so a file that does is refused before it is read. Our .glb keeps everything in its own binary chunk.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { outsideAddresses } from '../viewer/js/glb.js';

function glb(json) {
  let text = JSON.stringify(json);
  while (text.length % 4) text += ' ';
  const body = new TextEncoder().encode(text), out = new Uint8Array(20 + body.length), v = new DataView(out.buffer);
  v.setUint32(0, 0x46546c67, true); v.setUint32(4, 2, true); v.setUint32(8, out.length, true);
  v.setUint32(12, body.length, true); v.setUint32(16, 0x4e4f534a, true); out.set(body, 20);
  return out.buffer;
}

test('a glb with its buffer inside and pictures as data names nothing outside', () => {
  const file = glb({ asset: { version: '2.0' }, buffers: [{ byteLength: 8 }], images: [{ uri: 'data:image/png;base64,AA==' }, { bufferView: 0 }] });
  assert.deepEqual(outsideAddresses(file), []);
});

test('a buffer or picture at another address is found, in a glb or a gltf', () => {
  const json = { asset: { version: '2.0' }, buffers: [{ uri: 'https://example.com/a.bin', byteLength: 8 }], images: [{ uri: 'pic.png' }] };
  assert.deepEqual(outsideAddresses(glb(json)), ['https://example.com/a.bin', 'pic.png']);
  assert.deepEqual(outsideAddresses(new TextEncoder().encode(JSON.stringify(json)).buffer), ['https://example.com/a.bin', 'pic.png']);
});

test('a file that is not glTF names nothing, and the loader says what is wrong with it', () => {
  assert.deepEqual(outsideAddresses(new Uint8Array([1, 2, 3]).buffer), []);
  assert.deepEqual(outsideAddresses(glb({ buffers: 'no' })), []);
});
