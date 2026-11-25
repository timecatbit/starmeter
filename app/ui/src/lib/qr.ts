// A QR code for the share picture's signature, so a reader can scan their way to the project. Byte mode at error
// correction level M or Q, the smallest version (1–10) that holds the text, the mask with the lowest penalty: the
// standard's algorithm, written out here rather than adding a library for one short URL.

export type Level = 'M' | 'Q';
/** Per level: error correction codewords per block and the number of blocks for versions 1–10, and its format bits.
 *  Q recovers a quarter of the symbol, enough to set a logo in the middle. */
const LEVELS: Record<Level, { ecc: number[]; blocks: number[]; bits: number }> = {
  M: { ecc: [0, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26], blocks: [0, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5], bits: 0 },
  Q: { ecc: [0, 13, 22, 18, 26, 18, 24, 18, 22, 20, 24], blocks: [0, 1, 1, 2, 2, 4, 4, 6, 6, 8, 8], bits: 3 },
};

const rawModules = (version: number) => {
  let result = (16 * version + 128) * version + 64;
  if (version >= 2) { const align = Math.floor(version / 7) + 2; result -= (25 * align - 10) * align - 55; if (version >= 7) result -= 36; }
  return result;
};
const dataCodewords = (version: number, level: Level) => Math.floor(rawModules(version) / 8) - LEVELS[level].ecc[version] * LEVELS[level].blocks[version];

function multiply(x: number, y: number): number {
  let z = 0;
  for (let i = 7; i >= 0; i--) { z = (z << 1) ^ ((z >>> 7) * 0x11d); z ^= ((y >>> i) & 1) * x; }
  return z;
}
function divisor(degree: number): number[] {
  const result = new Array<number>(degree).fill(0);
  result[degree - 1] = 1;
  let root = 1;
  for (let i = 0; i < degree; i++) {
    for (let j = 0; j < degree; j++) { result[j] = multiply(result[j], root); if (j + 1 < degree) result[j] ^= result[j + 1]; }
    root = multiply(root, 0x02);
  }
  return result;
}
function remainder(data: number[], by: number[]): number[] {
  const result = new Array<number>(by.length).fill(0);
  for (const b of data) {
    const factor = b ^ result.shift()!;
    result.push(0);
    by.forEach((coefficient, i) => (result[i] ^= multiply(coefficient, factor)));
  }
  return result;
}

/** The symbol's modules, true for dark, row by row; null when the text is too long for version 10. */
export function qrModules(text: string, level: Level = 'M'): boolean[][] | null {
  const bytes = [...new TextEncoder().encode(text)];
  let version = 1;
  const needed = (v: number) => 4 + (v < 10 ? 8 : 16) + bytes.length * 8;
  while (version <= 10 && needed(version) > dataCodewords(version, level) * 8) version++;
  if (version > 10) return null;
  const size = version * 4 + 17, capacity = dataCodewords(version, level) * 8;

  // The bit stream: mode, length, the bytes, a terminator, then padding.
  const bits: number[] = [];
  const append = (value: number, length: number) => { for (let i = length - 1; i >= 0; i--) bits.push((value >>> i) & 1); };
  append(0b0100, 4); append(bytes.length, version < 10 ? 8 : 16);
  for (const b of bytes) append(b, 8);
  append(0, Math.min(4, capacity - bits.length));
  append(0, (8 - (bits.length % 8)) % 8);
  for (let pad = 0xec; bits.length < capacity; pad ^= 0xec ^ 0x11) append(pad, 8);
  const data: number[] = [];
  for (let i = 0; i < bits.length; i += 8) data.push(bits.slice(i, i + 8).reduce((s, bit) => (s << 1) | bit, 0));

  // Split into blocks, add each block's error correction, and interleave.
  const blocks = LEVELS[level].blocks[version], eccLength = LEVELS[level].ecc[version], raw = Math.floor(rawModules(version) / 8);
  const short = blocks - (raw % blocks), shortLength = Math.floor(raw / blocks), by = divisor(eccLength);
  const pieces: number[][] = [];
  for (let i = 0, k = 0; i < blocks; i++) {
    const part = data.slice(k, k + shortLength - eccLength + (i < short ? 0 : 1));
    k += part.length;
    const ecc = remainder(part, by);
    if (i < short) part.push(0);
    pieces.push([...part, ...ecc]);
  }
  const codewords: number[] = [];
  for (let i = 0; i < pieces[0].length; i++) pieces.forEach((piece, j) => { if (i !== shortLength - eccLength || j >= short) codewords.push(piece[i]); });

  // Function patterns: timing, finders, alignment, format and version areas.
  const modules = Array.from({ length: size }, () => new Array<boolean>(size).fill(false));
  const fixed = Array.from({ length: size }, () => new Array<boolean>(size).fill(false));
  const set = (x: number, y: number, dark: boolean) => { modules[y][x] = dark; fixed[y][x] = true; };
  for (let i = 0; i < size; i++) { set(6, i, i % 2 === 0); set(i, 6, i % 2 === 0); }
  for (const [cx, cy] of [[3, 3], [size - 4, 3], [3, size - 4]]) {
    for (let dy = -4; dy <= 4; dy++) for (let dx = -4; dx <= 4; dx++) {
      const x = cx + dx, y = cy + dy, distance = Math.max(Math.abs(dx), Math.abs(dy));
      if (x >= 0 && x < size && y >= 0 && y < size) set(x, y, distance !== 2 && distance !== 4);
    }
  }
  const centres: number[] = [];
  if (version > 1) {
    const count = Math.floor(version / 7) + 2, step = Math.ceil((version * 4 + 4) / (count * 2 - 2)) * 2;
    centres.push(6);
    for (let at = size - 7; centres.length < count; at -= step) centres.splice(1, 0, at);
  }
  centres.forEach((y, i) => centres.forEach((x, j) => {
    if ((i === 0 && j === 0) || (i === 0 && j === centres.length - 1) || (i === centres.length - 1 && j === 0)) return;
    for (let dy = -2; dy <= 2; dy++) for (let dx = -2; dx <= 2; dx++) set(x + dx, y + dy, Math.max(Math.abs(dx), Math.abs(dy)) !== 1);
  }));
  const format = (mask: number) => {
    const value = (LEVELS[level].bits << 3) | mask;
    let rem = value;
    for (let i = 0; i < 10; i++) rem = (rem << 1) ^ ((rem >>> 9) * 0x537);
    const word = ((value << 10) | rem) ^ 0x5412, bit = (i: number) => ((word >>> i) & 1) === 1;
    for (let i = 0; i <= 5; i++) set(8, i, bit(i));
    set(8, 7, bit(6)); set(8, 8, bit(7)); set(7, 8, bit(8));
    for (let i = 9; i < 15; i++) set(14 - i, 8, bit(i));
    for (let i = 0; i < 8; i++) set(size - 1 - i, 8, bit(i));
    for (let i = 8; i < 15; i++) set(8, size - 15 + i, bit(i));
    set(8, size - 8, true);
  };
  format(0);
  if (version >= 7) {
    let rem = version;
    for (let i = 0; i < 12; i++) rem = (rem << 1) ^ ((rem >>> 11) * 0x1f25);
    const word = (version << 12) | rem;
    for (let i = 0; i < 18; i++) {
      const dark = ((word >>> i) & 1) === 1, a = size - 11 + (i % 3), b = Math.floor(i / 3);
      set(a, b, dark); set(b, a, dark);
    }
  }

  // The codewords in the zigzag, two columns at a time from the bottom right.
  let index = 0;
  for (let right = size - 1; right >= 1; right -= 2) {
    if (right === 6) right = 5;
    for (let vertical = 0; vertical < size; vertical++) {
      for (let j = 0; j < 2; j++) {
        const x = right - j, upward = ((right + 1) & 2) === 0, y = upward ? size - 1 - vertical : vertical;
        if (fixed[y][x] || index >= codewords.length * 8) continue;
        modules[y][x] = ((codewords[index >>> 3] >>> (7 - (index & 7))) & 1) === 1;
        index++;
      }
    }
  }

  // Every mask in turn; keep the one that leaves the fewest runs, blocks and imbalance.
  const flip = (mask: number, x: number, y: number) => [
    (x + y) % 2 === 0, y % 2 === 0, x % 3 === 0, (x + y) % 3 === 0, (Math.floor(x / 3) + Math.floor(y / 2)) % 2 === 0,
    ((x * y) % 2) + ((x * y) % 3) === 0, (((x * y) % 2) + ((x * y) % 3)) % 2 === 0, (((x + y) % 2) + ((x * y) % 3)) % 2 === 0,
  ][mask];
  const applyMask = (mask: number) => { for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) if (!fixed[y][x] && flip(mask, x, y)) modules[y][x] = !modules[y][x]; };
  const penalty = () => {
    let score = 0, dark = 0;
    for (let a = 0; a < size; a++) {
      for (const line of [(i: number) => modules[a][i], (i: number) => modules[i][a]]) {
        let run = 1;
        for (let i = 1; i <= size; i++) {
          if (i < size && line(i) === line(i - 1)) run++;
          else { if (run >= 5) score += run - 2; run = 1; }
        }
      }
    }
    for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
      if (modules[y][x]) dark++;
      if (x < size - 1 && y < size - 1 && modules[y][x] === modules[y][x + 1] && modules[y][x] === modules[y + 1][x] && modules[y][x] === modules[y + 1][x + 1]) score += 3;
    }
    return score + Math.floor(Math.abs(dark * 20 - size * size * 10) / (size * size)) * 10;
  };
  let best = 0, lowest = Infinity;
  for (let mask = 0; mask < 8; mask++) {
    applyMask(mask); format(mask);
    const score = penalty();
    if (score < lowest) { lowest = score; best = mask; }
    applyMask(mask);
  }
  applyMask(best); format(best);
  return modules;
}

/** The QR code in the logo's look, laid out in modules (drawn by 在手机上看, and by the share picture without the icon):
 *  a dot for each dark module outside the three finder eyes and the middle, the eyes' corners (drawn as rounded
 *  squares), and the middle square left clear for the app icon (level Q recovers the modules it covers). Without the
 *  icon every module outside the eyes is a dot. */
export function qrLook(modules: boolean[][], icon = true) {
  const n = modules.length, logo = n >= 29 ? 7 : 5, clear = icon ? logo + 2 : 0, from = Math.floor((n - clear) / 2);
  const eye = (mx: number, my: number) => (mx < 7 && my < 7) || (mx >= n - 7 && my < 7) || (mx < 7 && my >= n - 7);
  const centre = (mx: number, my: number) => mx >= from && mx < from + clear && my >= from && my < from + clear;
  const dots: [number, number][] = [];
  modules.forEach((row, my) => row.forEach((dark, mx) => { if (dark && !eye(mx, my) && !centre(mx, my)) dots.push([mx, my]); }));
  const eyes: [number, number][] = [[0, 0], [n - 7, 0], [0, n - 7]];
  return { n, dots, eyes, icon: { at: from + 1, size: logo } };
}
