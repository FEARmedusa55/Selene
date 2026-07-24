// Renders the Selene pixel-art moon to a PNG (no deps: manual PNG encoder).
const fs = require("fs");
const zlib = require("zlib");

const N = 32;            // logical pixel-art grid
const SCALE = 32;        // -> 1024x1024 output
const OUT = process.argv[2] || "app-icon.png";

const C = {
  bgTop:    [0xa9, 0x5f, 0xf2],
  bgBottom: [0x4f, 0x6c, 0xf2],
  outline:  [0x1d, 0x10, 0x33],
  light:    [0xcd, 0xc4, 0xdc],
  white:    [0xff, 0xff, 0xff],
  crater:   [0x8f, 0x86, 0xab],
  shadow:   [0xa2, 0x4e, 0xcd],
  craterS:  [0x8a, 0x3f, 0xb4],
};

const cx = 16, cy = 16;
const rOut = 9.1;   // outer edge of the dark outline
const rFill = 8.0;  // inner edge of the outline

// Lit disc: offset up-left, so the shadow reads as a crescent bottom-right.
const lcx = 13.85, lcy = 11.64, lr = 9.89;

// Craters are hand-placed blocks, not rasterised circles: at this grid size a
// circle quantises into a cross. [x, y, w, h]
const craters = [
  [14, 9, 2, 2],
  [19, 10, 2, 2],
  [16, 15, 2, 2],
  [10, 18, 2, 2],
  [9, 13, 1, 1],
  [20, 18, 2, 2],
  [15, 21, 2, 2],
];
// The bright kick each crater throws below itself. [x, y, w, h]
const craterRims = [
  [14, 11, 2, 1], [13, 10, 1, 1],
  [19, 12, 2, 1],
  [16, 17, 2, 1], [15, 16, 1, 1],
  [10, 20, 2, 1], [12, 19, 1, 1],
  [9, 14, 1, 1],
];

const dist = (x, y, ox, oy) => Math.hypot(x - ox, y - oy);
const inRects = (gx, gy, rects) =>
  rects.some(([x, y, w, h]) => gx >= x && gx < x + w && gy >= y && gy < y + h);

function cell(gx, gy) {
  const x = gx + 0.5, y = gy + 0.5;
  const d = dist(x, y, cx, cy);

  if (d > rOut) {
    const t = gy / (N - 1);
    return C.bgTop.map((v, i) => Math.round(v + (C.bgBottom[i] - v) * t));
  }
  if (d > rFill) return C.outline;

  const lit = dist(x, y, lcx, lcy) < lr;

  if (inRects(gx, gy, craters)) return lit ? C.crater : C.craterS;
  if (lit && inRects(gx, gy, craterRims)) return C.white;

  // Bright rim hugging the inside of the outline, on the lit side only.
  if (lit && d > rFill - 0.95) return C.white;

  return lit ? C.light : C.shadow;
}

// --- render ---------------------------------------------------------------
const W = N * SCALE;
const raw = Buffer.alloc((W * 4 + 1) * W);
const grid = [];
for (let gy = 0; gy < N; gy++) {
  grid[gy] = [];
  for (let gx = 0; gx < N; gx++) grid[gy][gx] = cell(gx, gy);
}
// The gradient plate is masked to a disc. That mask is the one thing not on the
// pixel grid: it is antialiased at output resolution, so the edge stays smooth
// when the OS scales the icon down to 16px.
const mid = W / 2, maskR = W / 2;
for (let y = 0; y < W; y++) {
  const row = y * (W * 4 + 1);
  raw[row] = 0; // filter: none
  const g = grid[Math.floor(y / SCALE)];
  for (let x = 0; x < W; x++) {
    const p = g[Math.floor(x / SCALE)];
    const o = row + 1 + x * 4;
    const edge = maskR - Math.hypot(x + 0.5 - mid, y + 0.5 - mid);
    raw[o] = p[0]; raw[o + 1] = p[1]; raw[o + 2] = p[2];
    raw[o + 3] = Math.round(255 * Math.max(0, Math.min(1, edge)));
  }
}

// --- PNG container --------------------------------------------------------
const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(W, 0);
ihdr.writeUInt32BE(W, 4);
ihdr[8] = 8;  // bit depth
ihdr[9] = 6;  // RGBA
fs.writeFileSync(OUT, Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk("IHDR", ihdr),
  chunk("IDAT", zlib.deflateSync(raw, { level: 9 })),
  chunk("IEND", Buffer.alloc(0)),
]));
console.log(`wrote ${OUT} (${W}x${W})`);
