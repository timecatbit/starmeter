// How figures read, as the 5.x panel wrote them: shares, token counts, money, Credits and the counts line.
import type { MoneyUnit, ModelUsage, ProviderUsage } from './types';

const fixed = (value: number, digits: number) => value.toFixed(digits);

/** A share as a reader expects it: whole percents, a decimal near 0 and 100, and a sliver never shown as 0.0% or 100.0%. */
export function share(ratio: number): string {
  const value = ratio * 100;
  if (value > 0 && value < 0.05) return '<0.1%';
  if (value > 99.95 && value < 100) return '>99.9%';
  return fixed(value, (value > 0 && value < 1) || (value > 99 && value < 100) ? 1 : 0) + '%';
}

export function tokens(value: number): string {
  if (value >= 1e9) return fixed(value / 1e9, 2) + 'B';
  if (value >= 1e6) return fixed(value / 1e6, 2) + 'M';
  if (value >= 1e3) return fixed(value / 1e3, 1) + 'K';
  return fixed(value, 0);
}

export function axisTokens(value: number): string {
  for (const [unit, suffix] of [[1e9, 'B'], [1e6, 'M'], [1e3, 'K']] as const) {
    if (value >= unit) {
      const scaled = value / unit;
      return (scaled >= 10 || scaled === Math.floor(scaled) ? fixed(scaled, 0) : fixed(scaled, 1)) + suffix;
    }
  }
  return fixed(value, 0);
}

/** Claude's family and version (claude-opus-5-5-20261101 → Opus 5.5); other names as they are. */
export function modelName(name: string): string {
  if (!name.startsWith('claude-')) return name;
  const value = name.replace(/-20\d{6}$/, '');
  const parts = value.slice(7).split('-');
  const family = parts[0];
  if (!['opus', 'sonnet', 'haiku', 'fable'].includes(family)) return name;
  return family[0].toUpperCase() + family.slice(1) + ' ' + parts.slice(1).join('.');
}

/** API-equivalent estimates read in dollars, or in yuan at the rate the dashboard carries. */
export const currency = {
  unit: null as MoneyUnit | null,
  get symbol() { return this.unit?.symbol ?? '$'; },
  convert(value: number) { return value * (this.unit?.rate ?? 1); },
};

export function money(value: number | null | undefined): string {
  return value === null || value === undefined ? '—' : currency.symbol + fixed(currency.convert(value), 2);
}

/** Dollars with thousands separators, for the share picture. */
export function grouped(value: number): string {
  return currency.convert(value).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
}

export const creditsText = (value: number) => (value >= 100 ? fixed(value, 0) : fixed(value, 1)) + ' Credits';

export function headline(p: ProviderUsage): string {
  if (p.cost === null && p.credits !== null) return creditsText(p.credits);
  return money(p.cost);
}

export const tokenLine = (p: ProviderUsage) => (p.tokens === 0 && p.credits !== null ? 'Token 未记录' : tokens(p.tokens) + ' Token');

export function modelAmount(m: ModelUsage): string {
  if (m.tokens === 0 && m.credits !== undefined) return creditsText(m.credits);
  return tokens(m.tokens);
}

export const isGrouped = (p: ProviderUsage) => p.id.startsWith('model:') || p.id.startsWith('project:');
export const colourOf = (p: ProviderUsage) => p.accent ?? p.id;

/** What Summary adds for a source: nothing when it isn't counted, only what no gateway logged when one counts the rest. */
export function summaryPart(p: ProviderUsage): ProviderUsage | null {
  if (p.summaryIncluded === false) return null;
  const c = p.counted;
  if (!c) return p;
  return { ...p, cost: c.cost, tokens: c.tokens, requests: c.requests, unpriced: c.unpriced, cache: c.cache, history: c.history };
}

/** The counts line, longest first: a card that can't fit it rounds the cache share, drops output, then sessions. */
export function statsForms(p: ProviderUsage): string[] {
  if (!p.available) return ['等待可用数据'];
  if (p.tokens === 0 && p.credits !== null) return [`${p.requests} 请求 · ${p.sessions} 会话`, `${p.requests} 请求`];
  const requests = `${p.requests} 请求`, output = `输出 ${tokens(p.output)}`;
  const cache = (digits: number) => (p.cache === null ? '缓存 —' : `缓存 ${fixed(p.cache * 100, digits)}%`);
  const extras: string[] = [];
  if (p.id === 'opencodex') { if (p.coverage !== null && p.coverage < 1) extras.push(`已定价 ${fixed(p.coverage * 100, 0)}%`); }
  else if (!isGrouped(p)) extras.push(`${p.sessions} 会话`);
  if (p.cost !== null && p.credits !== null) extras.push(creditsText(p.credits));
  const forms: string[] = [];
  for (const parts of [[requests, cache(1), output, ...extras], [requests, cache(0), output, ...extras], [requests, cache(0), ...extras], [requests, cache(0)], [requests]]) {
    const line = parts.join(' · ');
    if (!forms.includes(line)) forms.push(line);
  }
  return forms;
}

/** Normal states say nothing; a problem, or a note that changes how to read the numbers, sits after the name. */
export function notes(p: ProviderUsage): [string, boolean][] {
  if (!p.available) return [[p.status === '运行正常' || p.status === '已同步' ? '数据暂不可用' : p.status, true]];
  if (p.status !== '运行正常' && p.status !== '已同步') return [[p.status, true]];
  return [...(p.subtitle ? [[p.subtitle, false] as [string, boolean]] : []), ...(p.serviceNote ? [[p.serviceNote, true] as [string, boolean]] : []),
    ...(p.priceNote ? [[p.priceNote, true] as [string, boolean]] : [])];
}

/** A gateway's clients in one line, by their share of its tokens. */
export function clientLine(p: ProviderUsage): string | null {
  if (!p.available || !p.clients?.length) return null;
  return '客户端 · ' + p.clients.map((c) => c.name + ' ' + share(c.share)).join(' · ');
}

/** Legend names drop suffixes the cards below already show. */
export const legendName = (p: ProviderUsage) => (p.id === 'claude' ? 'Claude' : p.name.endsWith(' Harness') ? p.name.slice(0, -8) : p.name);


/** The figure split for drawing: the currency sign and K/M/B unit are set smaller and lighter than the digits. */
export function figureParts(value: string): { sign: string; body: string; unit: string } {
  let body = value, sign = '', unit = '';
  if (body && '$¥'.includes(body[0])) { sign = body[0]; body = body.slice(1); }
  const last = body[body.length - 1];
  if (last && 'KMB'.includes(last) && /\d/.test(body[body.length - 2] ?? '')) { unit = last; body = body.slice(0, -1); }
  return { sign, body, unit };
}
