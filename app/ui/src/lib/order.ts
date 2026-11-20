// The order of the tool cards: as the 模型 and 项目 tabs order theirs (the core's ranked groups), the costliest first,
// then the most tokens, then the most Credits. Tools with nothing in the range keep their own order after them.

type Usage = { cost: number | null; tokens: number; credits: number | null; requests: number };

export function byUsage<T extends Usage>(list: T[]): T[] {
  const used = (p: T) => p.requests > 0 || p.tokens > 0 || (p.credits ?? 0) > 0;
  const key = (p: T) => [p.cost ?? 0, p.tokens, p.credits ?? 0];
  return list
    .map((p, i) => ({ p, i }))
    .sort((a, b) => {
      const ua = used(a.p), ub = used(b.p);
      if (ua !== ub) return ua ? -1 : 1;
      if (ua) {
        const ka = key(a.p), kb = key(b.p);
        for (let k = 0; k < ka.length; k++) if (ka[k] !== kb[k]) return kb[k] - ka[k];
      }
      return a.i - b.i;
    })
    .map(({ p }) => p);
}
