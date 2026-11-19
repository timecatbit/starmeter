// Text widths, where a layout depends on them (the legend's columns, a value's shorter form, the chart's label column).

export const SANS = "-apple-system, BlinkMacSystemFont, 'Segoe UI Variable Text', 'Segoe UI', 'Microsoft YaHei UI', 'PingFang SC', system-ui, sans-serif";
export const ROUNDED = `ui-rounded, 'SF Pro Rounded', ${SANS}`;
export const MONO = "ui-monospace, 'SF Mono', Menlo, Consolas, 'Cascadia Mono', monospace";

let context: CanvasRenderingContext2D | null = null;
const cache = new Map<string, number>();

export function font(size: number, weight = 400, family = SANS): string {
  return `${weight} ${size}px ${family}`;
}

/** The drawn width of a string in a CSS font, rounded up as AppKit's layout did. */
export function width(text: string, cssFont: string): number {
  const key = cssFont + '\u0001' + text;
  const known = cache.get(key);
  if (known !== undefined) return known;
  context ??= document.createElement('canvas').getContext('2d');
  if (!context) return text.length * 7;
  context.font = cssFont;
  const value = Math.ceil(context.measureText(text).width);
  if (cache.size > 4000) cache.clear();
  cache.set(key, value);
  return value;
}
