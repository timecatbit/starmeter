<script lang="ts">
  // A tool's own page, opened from its card: what it used in the range (figures, notes, its own trend, every model, a
  // gateway's clients), then its settings as the module grid has them (where it reads from, what to open).
  import { chartPositions, smoothCurves, svgPath } from '../lib/chart';
  import { accent, alpha } from '../lib/colors';
  import { clientLine, colourOf, headline, modelAmount, modelName, notes, share, tokens } from '../lib/format';
  import { moveWindow, panel } from '../lib/ui.svelte';
  import type { Dashboard, Entry, ProviderUsage } from '../lib/types';
  import DetailList from './DetailList.svelte';
  import Glyph from './Glyph.svelte';
  import HintButton from './HintButton.svelte';
  import Scroller from './Scroller.svelte';

  let { p, dashboard, settings, busy, scrollTop = $bindable(0), onback, onopen, onchoose, onheight }: {
    p: ProviderUsage; dashboard: Dashboard; settings: Entry | null; busy: string[] | null; scrollTop?: number;
    onback: () => void; onopen: (e: Entry) => void; onchoose: (e: Entry) => void; onheight: (height: number) => void;
  } = $props();

  let open = $state(new Set<string>());
  const colour = $derived(accent(colourOf(p), panel.dark));
  const credits = $derived(p.tokens === 0 && p.credits !== null);
  const figures = $derived<[string, string][]>(p.available ? [
    credits ? ['Credits', String(Math.round(p.credits ?? 0))] : ['Token', tokens(p.tokens)],
    ['请求', String(p.requests)],
    ...(credits ? [] : [['缓存', p.cache === null ? '—' : (p.cache * 100).toFixed(1) + '%'], ['输出', tokens(p.output)]] as [string, string][]),
    ['会话', String(p.sessions)],
  ] : []);
  const said = $derived(notes(p));
  const clients = $derived(clientLine(p));

  // Its own trend over the range, on its own scale.
  const W = $derived(panel.width - 32), H = 64;
  const trend = $derived.by(() => {
    const n = dashboard.dates.length, values = Array.from({ length: n }, (_, i) => Math.max(0, p.history[i] ?? 0));
    if (n < 2 || !values.some((v) => v > 0)) return null;
    const top = Math.max(...values) * 1.1, plot = { x: 0, y: 4, w: W, h: H - 8 };
    const curve = smoothCurves([values], chartPositions(dashboard), plot, top).at(-1)!;
    return { line: svgPath(curve), area: svgPath(curve) + `L${W},${H - 4}L0,${H - 4}Z`, end: curve.end, first: dashboard.dates[0], last: dashboard.dates[n - 1] };
  });
  const gradient = `tool-${Math.random().toString(36).slice(2, 8)}`;
</script>

<div class="page">
  <!-- Pinned, the header's free space moves the window, as a title bar would. -->
  <div class="grip" class:movable={panel.pinned} aria-hidden="true"
    onpointerdown={(event) => { if (event.button === 0 && panel.pinned) { event.preventDefault(); void moveWindow(); } }}></div>
  <span class="back"><HintButton icon="chevron.left" hint="返回" label="返回" onclick={onback} /></span>
  <span class="page-glyph"><Glyph image={p.image} template={p.template} symbol={p.symbol} size={18} /></span>
  <h2 class="title">{p.name}</h2>
  <span class="value num">{p.available ? headline(p) : '—'}</span>
  <span class="rule"></span>
  <div class="body">
    <Scroller bind:scrollTop onswipeback={onback} onheight={(h) => onheight(50 + h + 12)}>
      <div class="content">
        <span class="range">{dashboard.rangeLabel}</span>
        {#if figures.length}
          <div class="figures">
            {#each figures as [name, value] (name)}
              <div class="figure"><span class="figure-name">{name}</span><span class="figure-value num">{value}</span></div>
            {/each}
          </div>
        {:else}
          <p class="quiet">等待可用数据</p>
        {/if}
        {#each said as [text, warning] (text)}
          <p class="note" class:warning>{text}</p>
        {/each}
        {#if trend}
          <h3 class="section">{dashboard.chartUnit} Token</h3>
          <svg class="trend" width={W} height={H} aria-hidden="true">
            <defs><linearGradient id={gradient} x1="0" y1="0" x2="0" y2="1">
              <stop offset="0" stop-color={alpha(colour, 0.22)} /><stop offset="1" stop-color={alpha(colour, 0)} />
            </linearGradient></defs>
            <path d={trend.area} fill="url(#{gradient})" />
            <path d={trend.line} fill="none" stroke={colour} stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
            <circle cx={trend.end[0]} cy={trend.end[1]} r="2.6" fill={colour} />
          </svg>
          <div class="ends"><span>{trend.first}</span><span>{trend.last}</span></div>
        {/if}
        {#if p.available && p.models.length}
          <h3 class="section">模型</h3>
          {#each p.models as m, i (m.name + i)}
            <div class="model">
              <span class="model-name">{modelName(m.name)}{#if m.priceNote}<span class="price">{m.priceNote}</span>{/if}</span>
              <span class="model-amount num">{modelAmount(m)}</span>
              <span class="model-share num">{share(m.share)}</span>
              <span class="track"><span class="fill" style:width="{Math.max(1.5, Math.min(1, m.share) * 100)}%" style:background={colour}></span></span>
            </div>
          {/each}
        {/if}
        {#if clients}<p class="quiet clients">{clients}</p>{/if}
        {#if settings?.children.length}
          <h3 class="section">设置</h3>
          <div class="settings">
            <DetailList entries={settings.children} {open} {busy} rowWidth={panel.width - 16} {onopen} {onchoose} ontoggled={(next) => (open = next)} />
          </div>
        {/if}
      </div>
    </Scroller>
  </div>
</div>

<style>
  .page {
    position: relative;
    width: 100%;
    height: 100%;
  }
  .grip { position: absolute; left: 0; top: 0; width: 100%; height: 48px; }
  .grip.movable { cursor: grab; }
  .grip.movable:active { cursor: grabbing; }
  .back { position: absolute; left: 10px; top: 11px; }
  .page-glyph { position: absolute; left: 48px; top: 16px; pointer-events: none; }
  .title {
    position: absolute;
    left: 73px;
    right: 110px;
    top: 15px;
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    line-height: 20px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    pointer-events: none;
  }
  .value {
    position: absolute;
    right: 16px;
    top: 16px;
    font-size: 14px;
    font-weight: 600;
    line-height: 18px;
    pointer-events: none;
  }
  .rule { position: absolute; left: 16px; right: 16px; top: 49px; height: 1px; background: var(--separator); }
  .body { position: absolute; left: 0; right: 0; top: 50px; bottom: 0; display: flex; }
  .content { padding: 10px 16px 4px; }
  .range { display: block; font-size: 10.5px; color: var(--secondary); margin-bottom: 6px; }
  .figures { display: grid; grid-template-columns: repeat(3, 1fr); gap: 6px; margin-bottom: 8px; }
  .figure {
    min-width: 0;
    padding: 7px 8px;
    border-radius: 9px;
    background: color-mix(in srgb, var(--label) 5%, transparent);
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .figure-name { font-size: 10px; color: var(--secondary); }
  .figure-value { font-size: 15px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .note { margin: 0 0 4px; font-size: 11px; color: var(--secondary); }
  .note.warning { color: var(--orange); }
  .quiet { margin: 4px 0; font-size: 11px; color: var(--secondary); }
  .clients { margin-top: 8px; }
  .section { margin: 14px 0 6px; font-size: 11px; font-weight: 600; color: var(--secondary); }
  .trend { display: block; overflow: visible; }
  .ends { display: flex; justify-content: space-between; font-size: 9.5px; color: var(--secondary); margin-top: 2px; }
  .model {
    display: grid;
    grid-template-columns: 1fr auto 40px;
    column-gap: 10px;
    align-items: baseline;
    padding: 4px 0 7px;
    position: relative;
  }
  .model-name { font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .price { margin-left: 5px; font-size: 9.5px; color: var(--orange); }
  .model-amount { font-size: 11.5px; text-align: right; }
  .model-share { font-size: 10.5px; color: var(--secondary); text-align: right; }
  .track {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 2px;
    height: 3px;
    border-radius: 1.5px;
    background: color-mix(in srgb, var(--label) 7%, transparent);
  }
  .fill { position: absolute; left: 0; top: 0; bottom: 0; border-radius: 1.5px; }
  .settings { margin: 0 -8px; }
</style>
