// The panel's colours. Text and fills come from CSS variables (app.css) that follow light and dark; the colours code
// needs by value (chart lines, canvases, the share pictures) are here, as the 5.x panel picked them.

const rgb = (r: number, g: number, b: number) => `rgb(${Math.round(r * 255)}, ${Math.round(g * 255)}, ${Math.round(b * 255)})`;

/** The system colours AppKit gives, light and dark. */
const system = {
  purple: ['rgb(175, 82, 222)', 'rgb(191, 90, 242)'],
  pink: ['rgb(255, 45, 85)', 'rgb(255, 55, 95)'],
  orange: ['rgb(255, 149, 0)', 'rgb(255, 159, 10)'],
  green: ['rgb(40, 205, 65)', 'rgb(50, 215, 75)'],
  teal: ['rgb(0, 195, 208)', 'rgb(0, 210, 224)'],
  indigo: ['rgb(88, 86, 214)', 'rgb(94, 92, 230)'],
  yellow: ['rgb(255, 204, 0)', 'rgb(255, 214, 10)'],
  red: ['rgb(255, 59, 48)', 'rgb(255, 69, 58)'],
} as const;
export const systemColor = (name: keyof typeof system, dark: boolean) => system[name][dark ? 1 : 0];

/** Each tool's colour on the chart, its card and the share picture. */
export function accent(id: string, dark: boolean): string {
  switch (id) {
    case 'claude': return dark ? rgb(0.91, 0.65, 0.46) : rgb(0.75, 0.4, 0.25);
    case 'codex': return systemColor('purple', dark);
    case 'opencode': return systemColor('pink', dark);
    case 'workbuddy': return systemColor('orange', dark);
    case 'qoder': return systemColor('green', dark);
    case 'gemini': return systemColor('teal', dark);
    case 'deepseek-harness': return systemColor('indigo', dark);
    case 'magpie': return systemColor('yellow', dark);
    case 'cursor': return dark ? rgb(0.7, 0.74, 0.8) : rgb(0.33, 0.37, 0.43);
    default: return dark ? rgb(0.55, 0.71, 0.94) : rgb(0.32, 0.51, 0.78);
  }
}

/** Summary's total: the soft teal of the mark's night sky. */
export const totalColor = (dark: boolean) => (dark ? rgb(0.43, 0.79, 0.75) : rgb(0.15, 0.47, 0.46));
/** The panel's accent, as magpie's: a soft indigo, periwinkle in the dark. */
export const starAccent = (dark: boolean) => (dark ? rgb(0.647, 0.706, 0.988) : rgb(0.345, 0.31, 0.89));
export const statusGreen = (dark: boolean) => (dark ? rgb(0.49, 0.79, 0.61) : rgb(0.18, 0.49, 0.31));
/** The label colour at an opacity, as AppKit's labelColor.withAlphaComponent. */
export const label = (dark: boolean, alpha: number) => (dark ? `rgba(255, 255, 255, ${alpha})` : `rgba(0, 0, 0, ${alpha})`);

/** A colour at a given opacity; takes rgb(...) strings. */
export function alpha(colour: string, amount: number): string {
  const match = colour.match(/rgba?\(([^)]+)\)/);
  if (!match) return colour;
  const [r, g, b, a = '1'] = match[1].split(',').map((s) => s.trim());
  return `rgba(${r}, ${g}, ${b}, ${Number(a) * amount})`;
}

/** Part way from one colour to another, for a figure that glows in its tool's colour as it grows. */
export function blend(from: string, to: string, t: number): string {
  if (t <= 0) return from;
  const parse = (c: string) => (c.match(/rgba?\(([^)]+)\)/)?.[1].split(',').map((s) => Number(s.trim())) ?? [0, 0, 0, 1]);
  const [a, b] = [parse(from), parse(to)];
  const mix = (i: number) => (a[i] ?? 1) + ((b[i] ?? 1) - (a[i] ?? 1)) * t;
  return `rgba(${Math.round(mix(0))}, ${Math.round(mix(1))}, ${Math.round(mix(2))}, ${mix(3)})`;
}

/** The heatmap's five steps: empty, then four of the total's colour. */
export const heatmapShades = (dark: boolean) => [label(dark, dark ? 0.26 : 0.2), ...[0.55, 0.7, 0.85, 1].map((a) => alpha(totalColor(dark), a))];
/** A dot still to come: barely there, so the row runs on without the future reading as empty hours. */
export const heatmapFuture = (dark: boolean) => label(dark, dark ? 0.13 : 0.1);

/** A level's bar: the accent, orange from 70%, red from 90%. */
export const barColor = (level: string | undefined, dark: boolean) =>
  level === 'critical' ? systemColor('red', dark) : level === 'warning' ? systemColor('orange', dark) : starAccent(dark);

/** A figure's colour by usage level; null leaves the row's own colour. */
export const usageColor = (level: string | undefined) =>
  level === 'normal' ? 'var(--label)' : level === 'warning' ? 'var(--orange)' : level === 'critical' ? 'var(--red)' : null;
