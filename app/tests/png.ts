// A small PNG reader for screenshots (8-bit RGB or RGBA, no interlace), so a test can measure where ink falls.
import { inflateSync } from 'node:zlib';

export function decodePng(file: Buffer): { width: number; height: number; pixels: Uint8Array } {
  let offset = 8, width = 0, height = 0, channels = 4;
  const chunks: Buffer[] = [];
  while (offset < file.length) {
    const length = file.readUInt32BE(offset), type = file.toString('ascii', offset + 4, offset + 8), data = file.subarray(offset + 8, offset + 8 + length);
    if (type === 'IHDR') { width = data.readUInt32BE(0); height = data.readUInt32BE(4); channels = data[9] === 2 ? 3 : 4; }
    else if (type === 'IDAT') chunks.push(data);
    offset += 12 + length;
  }
  const raw = inflateSync(Buffer.concat(chunks)), stride = width * channels, pixels = new Uint8Array(width * height * 4);
  let previous = new Uint8Array(stride);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)], line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1)), out = new Uint8Array(stride);
    for (let i = 0; i < stride; i++) {
      const a = i >= channels ? out[i - channels] : 0, b = previous[i], c = i >= channels ? previous[i - channels] : 0;
      const paeth = () => { const p = a + b - c, pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c); return pa <= pb && pa <= pc ? a : pb <= pc ? b : c; };
      out[i] = (line[i] + [0, a, b, (a + b) >> 1, paeth()][filter]) & 255;
    }
    for (let x = 0; x < width; x++) {
      for (let k = 0; k < 3; k++) pixels[(y * width + x) * 4 + k] = out[x * channels + k];
      pixels[(y * width + x) * 4 + 3] = channels === 4 ? out[x * channels + 3] : 255;
    }
    previous = out;
  }
  return { width, height, pixels };
}
