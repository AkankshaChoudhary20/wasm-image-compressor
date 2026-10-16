import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
const html = readFileSync(new URL('../index.html', import.meta.url), 'utf8');
const source = html.match(/<script type="module">([\s\S]*?)<\/script>/)[1]
  .replace(/^\s*import .*?;\s*$/m, '');
async function page() {
  const elements = new Map();
  function element(id) {
    if (!elements.has(id)) elements.set(id, {
      files: [], value: id === 'format' ? 'jpeg' : '800', disabled: true,
      handlers: {}, children: [],
      addEventListener(name, handler) { this.handlers[name] = handler; },
      appendChild(child) { this.children.push(child); }
    });
    return elements.get(id);
  }
  const timers = [], compressed = [];
  vm.runInNewContext(source, {
    document: { getElementById: element, createElement: () => ({}) },
    init: async () => {}, init_panic_hook() {}, get_dimensions: () => [1, 1],
    compress_image(bytes) { compressed.push([...bytes]); return new Uint8Array([1]); },
    Uint8Array, Blob, URL: { createObjectURL: () => 'blob:preview' },
    performance: { now: () => 0 }, setTimeout: fn => timers.push(fn)
  });
  await new Promise(setImmediate);
  return { element, timers, compressed, select(file) {
    element('file-input').files = file ? [file] : [];
    return element('file-input').handlers.change();
  }, process() { element('process-btn').handlers.click(); timers.shift()(); } };
}
const file = (name, bytes) => ({ name, type: 'image/png', arrayBuffer: async () => new Uint8Array(bytes).buffer });
function pending(name) {
  let resolve;
  const promise = new Promise(r => { resolve = r; });
  return { file: { name, type: 'image/png', arrayBuffer: () => promise }, resolve };
}
test('a slow older selection cannot replace the latest bytes or filename', async () => {
  const p = await page(), old = pending('old.png');
  const oldRead = p.select(old.file);
  await p.select(file('new.png', [2, 3]));
  old.resolve(new Uint8Array([9]).buffer);
  await oldRead;
  p.process();
  assert.deepEqual(p.compressed, [[2, 3]]);
  assert.equal(p.element('download-wrap').children[0].download, 'new-compressed.jpg');
});
test('a queued process is invalidated while the next file is loading', async () => {
  const p = await page();
  await p.select(file('old.png', [1]));
  p.element('process-btn').handlers.click();
  const next = pending('next.png'), read = p.select(next.file);
  p.timers.shift()();
  assert.equal(p.compressed.length, 0);
  assert.equal(p.element('process-btn').disabled, true);
  next.resolve(new Uint8Array([4, 5]).buffer);
  await read;
  assert.equal(p.element('process-btn').disabled, false);
  p.process();
  assert.deepEqual(p.compressed, [[4, 5]]);
});
test('clearing a selection disables processing and hides the preview', async () => {
  const p = await page();
  await p.select(file('old.png', [1]));
  await p.select(null);
  assert.equal(p.element('process-btn').disabled, true);
  assert.equal(p.element('before-img').hidden, true);
});
test('read failures leave processing disabled and report the error', async () => {
  const p = await page();
  await p.select({ name: 'bad.png', arrayBuffer: async () => { throw Error('read failed'); } });
  assert.equal(p.element('process-btn').disabled, true);
  assert.match(p.element('status').textContent, /Could not read image.*read failed/);
});
