// The panel's icons, drawn after the SF Symbols the engine names (Entry.symbol) so the same names work on every
// platform: 20×20, a 1.6 stroke in the current colour; "fill" marks a shape filled instead of stroked.

type Icon = { d: string; fill?: string };
const icons: Record<string, Icon> = {
  'chart.xyaxis.line': { d: 'M3.5 3v13.5H17 M5.5 13l3.5-4 3 2.5 4.5-5.5' },
  'square.grid.3x3.fill': { d: '', fill: 'M3 3h3.6v3.6H3z M8.2 3h3.6v3.6H8.2z M13.4 3H17v3.6h-3.6z M3 8.2h3.6v3.6H3z M8.2 8.2h3.6v3.6H8.2z M13.4 8.2H17v3.6h-3.6z M3 13.4h3.6V17H3z M8.2 13.4h3.6V17H8.2z M13.4 13.4H17V17h-3.6z' },
  'square.grid.2x2': { d: 'M3.5 3.5h5v5h-5z M11.5 3.5h5v5h-5z M3.5 11.5h5v5h-5z M11.5 11.5h5v5h-5z' },
  network: { d: 'M10 3.2a2 2 0 1 1 0 4 2 2 0 0 1 0-4z M4.6 12.8a2 2 0 1 1 0 4 2 2 0 0 1 0-4z M15.4 12.8a2 2 0 1 1 0 4 2 2 0 0 1 0-4z M10 7.2v3 M10 10.2l-4.2 2.9 M10 10.2l4.2 2.9' },
  'server.rack': { d: 'M3.5 3.5h13v4h-13z M3.5 8h13v4h-13z M3.5 12.5h13v4h-13z M6 5.5h.01 M6 10h.01 M6 14.5h.01' },
  'arrow.down.circle': { d: 'M10 2.8a7.2 7.2 0 1 1 0 14.4 7.2 7.2 0 0 1 0-14.4z M10 6.2v7.2 M6.9 10.6l3.1 3 3.1-3' },
  'person.crop.circle': { d: 'M10 2.8a7.2 7.2 0 1 1 0 14.4 7.2 7.2 0 0 1 0-14.4z M10 6a2.6 2.6 0 1 1 0 5.2A2.6 2.6 0 0 1 10 6z M5.3 15.2c1-1.8 2.7-2.7 4.7-2.7s3.7.9 4.7 2.7' },
  creditcard: { d: 'M2.8 5h14.4v10H2.8z M2.8 8.2h14.4 M5.5 12.3h3' },
  folder: { d: 'M2.8 5.5c0-.8.6-1.4 1.4-1.4h3.4l1.6 1.8h6.6c.8 0 1.4.6 1.4 1.4v7.4c0 .8-.6 1.4-1.4 1.4H4.2c-.8 0-1.4-.6-1.4-1.4z' },
  'questionmark.folder': { d: 'M2.8 5.5c0-.8.6-1.4 1.4-1.4h3.4l1.6 1.8h6.6c.8 0 1.4.6 1.4 1.4v7.4c0 .8-.6 1.4-1.4 1.4H4.2c-.8 0-1.4-.6-1.4-1.4z M8.4 9.4a1.6 1.6 0 1 1 2.3 1.4c-.5.3-.7.6-.7 1.1 M10 13.6h.01' },
  'arrow.clockwise': { d: 'M15.6 10a5.6 5.6 0 1 1-1.9-4.2 M14.2 2.8v3.4h-3.4' },
  'arrow.counterclockwise': { d: 'M4.4 10a5.6 5.6 0 1 0 1.9-4.2 M5.8 2.8v3.4h3.4' },
  'arrow.uturn.backward': { d: 'M7.4 4.6 4 8l3.4 3.4 M4 8h7.6a4.4 4.4 0 0 1 0 8.8H9' },
  link: { d: 'M8.6 11.4a3 3 0 0 0 4.3 0l2.3-2.3a3 3 0 0 0-4.3-4.3l-.9.9 M11.4 8.6a3 3 0 0 0-4.3 0l-2.3 2.3a3 3 0 0 0 4.3 4.3l.9-.9' },
  key: { d: 'M7.2 9.6a3.4 3.4 0 1 1 0 .01z M10.2 11.2l6 0 M14.2 11.2v2.4 M16.2 11.2v1.8', fill: 'M6.3 9.6a.9.9 0 1 1 1.8 0 .9.9 0 0 1-1.8 0z' },
  'checkmark.shield': { d: 'M10 2.8l5.6 2.2v4.4c0 3.6-2.4 6.2-5.6 7.8-3.2-1.6-5.6-4.2-5.6-7.8V5z M7.4 10l1.9 1.9 3.4-3.6' },
  pencil: { d: 'M12.8 3.9l3.3 3.3-8.4 8.4-3.9.6.6-3.9z M11.2 5.5l3.3 3.3' },
  trash: { d: 'M3.6 5.6h12.8 M8 5.6V4h4v1.6 M5.2 5.6l.8 10.6c.1.7.6 1.2 1.3 1.2h5.4c.7 0 1.2-.5 1.3-1.2l.8-10.6 M8.4 8.6v5.6 M11.6 8.6v5.6' },
  macwindow: { d: 'M2.8 4.2h14.4v11.6H2.8z M2.8 7.4h14.4 M5 5.8h.01 M6.8 5.8h.01' },
  'checkmark.circle': { d: 'M10 2.8a7.2 7.2 0 1 1 0 14.4 7.2 7.2 0 0 1 0-14.4z M6.8 10.1l2.2 2.2 4.2-4.6' },
  'xmark.circle': { d: 'M10 2.8a7.2 7.2 0 1 1 0 14.4 7.2 7.2 0 0 1 0-14.4z M7.4 7.4l5.2 5.2 M12.6 7.4l-5.2 5.2' },
  'dot.radiowaves.left.and.right': { d: 'M6.6 6.6a4.8 4.8 0 0 0 0 6.8 M13.4 6.6a4.8 4.8 0 0 1 0 6.8 M4.2 4.2a8.2 8.2 0 0 0 0 11.6 M15.8 4.2a8.2 8.2 0 0 1 0 11.6', fill: 'M8.6 10a1.4 1.4 0 1 1 2.8 0 1.4 1.4 0 0 1-2.8 0z' },
  'square.and.arrow.down': { d: 'M7 7.6H5.4c-.6 0-1 .4-1 1v7c0 .6.4 1 1 1h9.2c.6 0 1-.4 1-1v-7c0-.6-.4-1-1-1H13 M10 2.8v9 M7.2 9.2l2.8 2.8 2.8-2.8' },
  'square.and.arrow.up': { d: 'M7 7.6H5.4c-.6 0-1 .4-1 1v7c0 .6.4 1 1 1h9.2c.6 0 1-.4 1-1v-7c0-.6-.4-1-1-1H13 M10 12.2v-9.4 M7.2 5.4 10 2.6l2.8 2.8' },
  'arrow.left.arrow.right': { d: 'M3.6 7h11.8 M12.6 4.2 15.4 7l-2.8 2.8 M16.4 13H4.6 M7.4 10.2 4.6 13l2.8 2.8' },
  'arrow.up.arrow.down': { d: 'M6.6 16V4.4 M3.8 7.2l2.8-2.8 2.8 2.8 M13.4 4v11.6 M10.6 12.8l2.8 2.8 2.8-2.8' },
  'sparkle.magnifyingglass': { d: 'M8.8 3.2a5.6 5.6 0 1 1 0 11.2 5.6 5.6 0 0 1 0-11.2z M12.9 12.9l3.9 3.9', fill: 'M8.8 5.6l.75 1.7 1.7.75-1.7.75-.75 1.7-.75-1.7-1.7-.75 1.7-.75z' },
  sparkles: { d: '', fill: 'M8 2.6l1.3 3.6 3.6 1.3-3.6 1.3L8 12.4 6.7 8.8 3.1 7.5l3.6-1.3z M14.4 10.6l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z' },
  globe: { d: 'M10 2.8a7.2 7.2 0 1 1 0 14.4 7.2 7.2 0 0 1 0-14.4z M2.8 10h14.4 M10 2.8c2 2 3 4.4 3 7.2s-1 5.2-3 7.2 M10 2.8c-2 2-3 4.4-3 7.2s1 5.2 3 7.2' },
  'arrow.up.right': { d: 'M6 14 14 6 M7.6 6H14v6.4' },
  gearshape: { d: 'M10 7.4a2.6 2.6 0 1 1 0 5.2 2.6 2.6 0 0 1 0-5.2z M8.6 2.8h2.8l.4 2 1.5.9 1.9-.7 1.4 2.4-1.5 1.4v1.7l1.5 1.4-1.4 2.4-1.9-.7-1.5.9-.4 2H8.6l-.4-2-1.5-.9-1.9.7-1.4-2.4 1.5-1.4V9.2L3.4 7.8l1.4-2.4 1.9.7 1.5-.9z' },
  pin: { d: 'M7.2 3.2h5.6 M8.2 3.2v4.6L5.6 11h8.8l-2.6-3.2V3.2 M10 11v5.8' },
  'pin.fill': { d: 'M7.2 3.2h5.6 M10 11v5.8', fill: 'M7.4 3.2h5.2v4.4l2.8 3.6H4.6l2.8-3.6z' },
  'chevron.left': { d: 'M12.2 4.4 6.6 10l5.6 5.6' },
  'chevron.right': { d: 'M7.8 4.4 13.4 10l-5.6 5.6' },
  'rectangle.compress.vertical': { d: 'M3.2 9.2h13.6 M3.2 11h13.6 M10 2.8v4 M7.8 4.8 10 7l2.2-2.2 M10 17.2v-4 M7.8 15.2 10 13l2.2 2.2' },
  'slider.horizontal.3': { d: 'M3 5.6h14 M3 10h14 M3 14.4h14', fill: 'M11.4 5.6a1.8 1.8 0 1 1 3.6 0 1.8 1.8 0 0 1-3.6 0z M5 10a1.8 1.8 0 1 1 3.6 0A1.8 1.8 0 0 1 5 10z M9.4 14.4a1.8 1.8 0 1 1 3.6 0 1.8 1.8 0 0 1-3.6 0z' },
  'dollarsign.circle': { d: 'M10 2.8a7.2 7.2 0 1 1 0 14.4 7.2 7.2 0 0 1 0-14.4z M12.2 7.6c-.3-.9-1.2-1.4-2.2-1.4-1.3 0-2.2.7-2.2 1.7 0 2.4 4.5 1.3 4.5 3.9 0 1.1-1 1.8-2.3 1.8-1.2 0-2.1-.6-2.4-1.5 M10 5v1.2 M10 13.6v1.2' },
  'yensign.circle': { d: 'M10 2.8a7.2 7.2 0 1 1 0 14.4 7.2 7.2 0 0 1 0-14.4z M7.2 5.8 10 9.6l2.8-3.8 M10 9.6v4.8 M7.6 10.6h4.8 M7.6 12.6h4.8' },
  'menubar.rectangle': { d: 'M2.8 4.2h14.4v11.6H2.8z M2.8 7.2h14.4 M11.2 5.7h3.6' },
  bell: { d: 'M5.2 13.6V9.2a4.8 4.8 0 0 1 9.6 0v4.4l1.4 1.6H3.8z M8.4 15.2a1.6 1.6 0 0 0 3.2 0' },
  'waveform.path.ecg': { d: 'M2.6 10.4h3.2l1.6-3.6 2.6 7.2 2-5.2 1.2 1.6h4.2' },
  archivebox: { d: 'M2.8 3.8h14.4v3.6H2.8z M4 7.4v8.2h12V7.4 M8 10.4h4' },
  'doc.text': { d: 'M5 2.8h6.4l3.6 3.6v10.8H5z M11.4 2.8v3.6H15 M7.4 10h5.2 M7.4 12.8h5.2' },
  'doc.on.doc': { d: 'M7 6.2h7.6v10.6H7z M5.4 13.6H4.2V3.2h7.4v1.4' },
  cpu: { d: 'M5.6 5.6h8.8v8.8H5.6z M8.2 8.2h3.6v3.6H8.2z M8 2.8v2.8 M12 2.8v2.8 M8 14.4v2.8 M12 14.4v2.8 M2.8 8h2.8 M2.8 12h2.8 M14.4 8h2.8 M14.4 12h2.8' },
  'ellipsis.circle': { d: 'M10 2.8a7.2 7.2 0 1 1 0 14.4 7.2 7.2 0 0 1 0-14.4z M6.6 10h.01 M10 10h.01 M13.4 10h.01' },
  calendar: { d: 'M3.2 4.6h13.6v11.8H3.2z M3.2 8h13.6 M6.6 2.8v3 M13.4 2.8v3' },
  'square.stack.3d.up.fill': { d: '', fill: 'M10 2.6l7 3.6-7 3.6-7-3.6z M4.4 9.4 10 12.3l5.6-2.9 1.4.8-7 3.6-7-3.6z M4.4 12.6 10 15.5l5.6-2.9 1.4.8-7 3.6-7-3.6z' },
  'bolt.fill': { d: '', fill: 'M11.4 2.4 4.6 11.2h4.6l-1 6.4 6.8-8.8h-4.6z' },
  checkmark: { d: 'M4.4 10.4l3.6 3.6 7.6-8' },
  exclamationmark: { d: 'M10 3.6v8.4 M10 15.8h.01' },
  terminal: { d: 'M2.8 4.2h14.4v11.6H2.8z M5.8 8l2.6 2-2.6 2 M10 12.4h4' },
  plus: { d: 'M10 4v12 M4 10h12' },
  minus: { d: 'M4 10h12' },
};

export const hasIcon = (name: string | undefined) => !!name && name in icons;

/** The icon's SVG markup, or a small dot for a name the panel doesn't draw yet. */
export function iconSvg(name: string): string {
  const icon = icons[name];
  if (!icon) return '<circle cx="10" cy="10" r="2.4" fill="currentColor"/>';
  return (icon.d ? `<path d="${icon.d}" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>` : '')
    + (icon.fill ? `<path d="${icon.fill}" fill="currentColor"/>` : '');
}

/** The same icon as an image, for canvases (the share pictures). */
export function iconImage(name: string, colour: string): HTMLImageElement {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" style="color:${colour}">${iconSvg(name).replaceAll('currentColor', colour)}</svg>`;
  const image = new Image();
  image.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(svg);
  return image;
}

const centres = new Map<string, Promise<[number, number]>>();

/** Where an icon's middle is as the eye sees it, in its 20×20 box: across, its shape's middle; down, halfway between its
 *  shape's middle and its ink's weight, so a tall shape with a heavy base (the share box) doesn't sit low and the pin
 *  doesn't sit high. Measured once per icon. */
export function eyeCentre(name: string): Promise<[number, number]> {
  let known = centres.get(name);
  if (!known) {
    known = measureEye(name).catch(() => [10, 10] as [number, number]);
    centres.set(name, known);
  }
  return known;
}

async function measureEye(name: string): Promise<[number, number]> {
  const scale = 8, side = 20 * scale;
  const image = new Image();
  image.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" width="${side}" height="${side}">${iconSvg(name).replaceAll('currentColor', '#000')}</svg>`);
  await image.decode();
  const canvas = document.createElement('canvas');
  canvas.width = canvas.height = side;
  const ctx = canvas.getContext('2d', { willReadFrequently: true });
  if (!ctx) return [10, 10];
  ctx.drawImage(image, 0, 0, side, side);
  const data = ctx.getImageData(0, 0, side, side).data;
  let minX = side, maxX = -1, minY = side, maxY = -1, mass = 0, sumY = 0;
  for (let y = 0; y < side; y++) {
    for (let x = 0; x < side; x++) {
      const alpha = data[(y * side + x) * 4 + 3] / 255;
      if (alpha <= 0.02) continue;
      minX = Math.min(minX, x); maxX = Math.max(maxX, x); minY = Math.min(minY, y); maxY = Math.max(maxY, y);
      mass += alpha; sumY += alpha * (y + 0.5);
    }
  }
  if (maxX < 0 || !mass) return [10, 10];
  return [(minX + maxX + 1) / 2 / scale, ((minY + maxY + 1) / 2 + sumY / mass) / 2 / scale];
}
