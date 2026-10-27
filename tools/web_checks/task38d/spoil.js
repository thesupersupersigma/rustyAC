// check.mjs --script: spoils the stored copy of bmw_z4_gt3's data.acd in the browser's storage
// (same size, every stored byte shifted: what a damaged copy looks like) and adds a file no pack
// lists (a leftover of an older pack). Returns what it did.
const manifest = await (await fetch('preview/manifest.json')).json();
const entry = manifest.files.find((f) => f.path.toLowerCase() === 'content/cars/bmw_z4_gt3/data.acd');
const dir = await (await navigator.storage.getDirectory()).getDirectoryHandle('rustyac-pack-1');
const handle = await dir.getFileHandle(entry.sha256);
const bytes = new Uint8Array(await (await handle.getFile()).arrayBuffer());
const view = new DataView(bytes.buffer);
let pos = view.getInt32(0, true) === -1111 ? 8 : 0;
let spoiled = 0;
while (pos + 4 <= bytes.length) {
  pos += 4 + view.getInt32(pos, true);
  const size = view.getInt32(pos, true);
  pos += 4;
  for (let i = 0; i < size; i++) bytes[pos + 4 * i] ^= 0x55;
  spoiled += size;
  pos += 4 * size;
}
const out = await handle.createWritable();
await out.write(bytes);
await out.close();
const junk = await (await dir.getFileHandle('0'.repeat(64), { create: true })).createWritable();
await junk.write(new Uint8Array(1000));
await junk.close();
return { file: entry.path, bytes: bytes.length, spoiled_bytes: spoiled, junk: '0'.repeat(64) };
