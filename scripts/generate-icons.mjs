// Draws reframe's icon (a rounded square holding a 2x2 grid of tiles) into PNGs without dependencies.
// Usage: node scripts/generate-icons.mjs
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';

const out = path.join(import.meta.dirname, '..', 'resources');
fs.mkdirSync(out, { recursive: true });

const BG = [32, 34, 40];
const TILE = [86, 92, 107];
const ACCENT = [217, 119, 87];

function roundedRect(px, py, x, y, w, h, r) {
  const cx = Math.min(Math.max(px, x + r), x + w - r);
  const cy = Math.min(Math.max(py, y + r), y + h - r);
  return (px - cx) ** 2 + (py - cy) ** 2 <= r * r && px >= x && px <= x + w && py >= y && py <= y + h;
}

function draw(size, { background = true } = {}) {
  const ss = 4; // supersampling per axis
  const data = Buffer.alloc(size * size * 4);
  const pad = background ? 0.06 : 0;
  const inner = background ? 0.2 : 0.06;
  const gap = 0.07;
  const cell = (1 - 2 * inner - gap) / 2;
  const tiles = [
    [inner, inner, ACCENT],
    [inner + cell + gap, inner, TILE],
    [inner, inner + cell + gap, TILE],
    [inner + cell + gap, inner + cell + gap, TILE],
  ];
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      let r = 0,
        g = 0,
        b = 0,
        a = 0;
      for (let sy = 0; sy < ss; sy++) {
        for (let sx = 0; sx < ss; sx++) {
          const u = (x + (sx + 0.5) / ss) / size;
          const v = (y + (sy + 0.5) / ss) / size;
          let color = null;
          if (background && roundedRect(u, v, pad, pad, 1 - 2 * pad, 1 - 2 * pad, 0.2)) color = BG;
          for (const [tx, ty, c] of tiles) if (roundedRect(u, v, tx, ty, cell, cell, cell * 0.18)) color = c;
          if (color) {
            r += color[0];
            g += color[1];
            b += color[2];
            a += 255;
          }
        }
      }
      const n = ss * ss;
      const covered = a / 255;
      const i = (y * size + x) * 4;
      data[i] = covered ? Math.round(r / covered) : 0;
      data[i + 1] = covered ? Math.round(g / covered) : 0;
      data[i + 2] = covered ? Math.round(b / covered) : 0;
      data[i + 3] = Math.round(a / n);
    }
  }
  return png(size, size, data);
}

function png(width, height, rgba) {
  const crcTable = Array.from({ length: 256 }, (_, n) => {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  });
  const crc = (buf) => {
    let c = 0xffffffff;
    for (const byte of buf) c = crcTable[(c ^ byte) & 0xff] ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  };
  const chunk = (type, body) => {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(body.length);
    const typed = Buffer.concat([Buffer.from(type, 'ascii'), body]);
    const sum = Buffer.alloc(4);
    sum.writeUInt32BE(crc(typed));
    return Buffer.concat([len, typed, sum]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8; // bit depth
  header[9] = 6; // RGBA
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) rgba.copy(raw, y * (width * 4 + 1) + 1, y * width * 4, (y + 1) * width * 4);
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', header),
    chunk('IDAT', zlib.deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

fs.writeFileSync(path.join(out, 'icon.png'), draw(512));
fs.writeFileSync(path.join(out, 'tray.png'), draw(16, { background: false }));
fs.writeFileSync(path.join(out, 'tray@2x.png'), draw(32, { background: false }));
console.log('icons written to', out);
