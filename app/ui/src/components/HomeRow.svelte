<script lang="ts">
  // A homepage row: its icon or status light, the name, the figure on the right (an accent pill, a configured light, or
  // coloured by usage), a thin usage bar under the name (for a plan with more than one limit, a bar per window, named:
  // five hours on the left and the week on the right, a line per model group), a detail line, and an arrow when it
  // opens a page.
  import { onPhone } from '../lib/backend';
  import { barColor, usageColor } from '../lib/colors';
  import { actionable, configured, gauge as hasGauge, usageShare } from '../lib/detail';
  import { font, width } from '../lib/measure';
  import { panel } from '../lib/ui.svelte';
  import type { Entry } from '../lib/types';
  import Glyph from './Glyph.svelte';
  import StatusLight from './StatusLight.svelte';

  let { entry, column, rowWidth = 344, onactivate }: { entry: Entry; column: number; rowWidth?: number; onactivate: (e: Entry) => void } = $props();

  const marked = $derived(entry.checked !== undefined && (entry.image !== undefined || entry.symbol !== undefined));
  const left = $derived(entry.section ? 12 : column + (marked ? 20 : 0));
  const canAct = $derived(actionable(entry));
  /** On a phone the rows' insides stay on the computer: they read as they do there, and don't open. */
  const full = $derived(canAct || onPhone);
  const trailing = $derived(entry.children.length ? 32 : 12);
  const labelFont = $derived(entry.section ? font(10.5, 500) : full ? font(12) : font(10.5));
  const valueWidth = (value: string) => width(value, font(11, 400)) + (entry.attention ? 16 : configured(value) !== null ? 12 : 0);
  /** A value too wide for its row gives way to its shorter forms, so neither the title nor the value is cut. */
  const value = $derived.by(() => {
    if (entry.value === undefined) return undefined;
    const room = rowWidth - left - trailing - 10 - width(entry.label, labelFont);
    for (const form of [entry.value, ...(entry.fallbacks ?? [])]) if (valueWidth(form) <= room) return form;
    return entry.fallbacks?.at(-1) ?? entry.value;
  });
  const usage = $derived(usageShare(entry));
  const gauge = $derived(onPhone ? usage !== null : hasGauge(entry));
  /** A plan with more than one limit: a line of bars per model group, five hours on the left and the week on the
   *  right, each named under it; Gemini 订阅's lines end with their group's logos (Gemini's; Claude's and OpenAI's).
   *  The row's figure is the tightest. */
  const lines = $derived.by(() => {
    const bars = entry.gauges ?? [];
    if (bars.length < 2) return null;
    const count = Math.max(...bars.map((bar) => Math.floor(bar.place / 2))) + 1;
    return Array.from({ length: count }, (_, line) => {
      const meters = [0, 1].map((column) => bars.find((bar) => bar.place === line * 2 + column));
      const named = meters.find((bar) => bar?.group);
      // The logos go at the end of the line's last name (the week's, or five hours' when it has no week).
      const last = meters[1] ? 1 : 0;
      return { group: named?.group, marks: named?.marks, last, meters };
    });
  });
  const configuredState = $derived(configured(value));
  const labelColor = $derived(entry.action?.[0] === 'stop' ? 'var(--red)' : full || entry.usageLevel !== undefined || entry.checked === true ? 'var(--label)' : 'var(--secondary)');
  const valueColor = $derived(usageColor(entry.usageLevel) ?? (full ? 'var(--label)' : 'var(--secondary)'));
</script>

{#if entry.section}
  <div class="section" style:padding-left="{left}px">{entry.label}</div>
{:else}
  <button class="row pressable" class:detailed={entry.detail !== undefined} class:tall={full} disabled={!canAct} data-key={entry.onOpenAction?.[1]}
    style:padding-left="{left}px" style:padding-right="{trailing}px" style:--title-size={full ? '12px' : '10.5px'}
    onclick={() => onactivate(entry)}>
    {#if entry.checked !== undefined}
      <span class="box" class:radio={entry.radio} class:on={entry.checked}>
        {#if entry.checked && !entry.radio}<svg viewBox="0 0 12 12" width="12" height="12"><path d="M2.5 6 5 8.8 9.5 3" fill="none" style="stroke: var(--on-accent)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>{/if}
        {#if entry.checked && entry.radio}<span class="pip"></span>{/if}
      </span>
      {#if entry.image || entry.symbol}<span class="icon" style:left="33px"><Glyph image={entry.image} template={entry.template} symbol={entry.symbol} size={14} tint={panel.dark ? 'rgb(230,230,230)' : 'rgb(46,46,46)'} /></span>{/if}
    {:else if entry.image || entry.symbol}
      <span class="icon" style:left="12px"><Glyph image={entry.image} template={entry.template} symbol={entry.symbol} size={14} tint={panel.dark ? 'rgb(230,230,230)' : 'rgb(46,46,46)'} /></span>
    {/if}
    <span class="block">
      <span class="title-line">
        {#if entry.statusLight}<span class="light" style:left="{19 - left - 3.5}px"><StatusLight state={entry.statusLight} /></span>{/if}
        <span class="title" style:color={labelColor}>{entry.label}</span>
        {#if value !== undefined}
          {#if entry.attention}
            <span class="value pill num">{value}</span>
          {:else if configuredState !== null}
            <span class="value configured num"><StatusLight state={configuredState ? 'ok' : 'unknown'} />{value}</span>
          {:else}
            <span class="value num" style:color={valueColor}>{value}</span>
          {/if}
        {/if}
      </span>
      {#if gauge && usage !== null && lines}
        <span class="pair">
          {#each lines as line, i (i)}
            {#each line.meters as bar, column (column)}
              <span class="half" style:grid-row={i + 1} style:grid-column={column + 1}>
                {#if bar}
                  <span class="gauge"><span class="gauge-fill" style:width="max(3px, {bar.used}%)" style:background={barColor(bar.level, panel.dark)}
                    style:--pen={barColor(bar.level, panel.dark)}></span></span>
                  <!-- A narrow panel shortens the window's name, never the share. -->
                  <span class="gauge-name"><span class="gauge-label">{bar.label}</span> <span class="num" style:color={usageColor(bar.level)}>{bar.used}%</span>
                    {#if line.marks?.length && column === line.last}
                      <span class="marks" role="img" title={line.group} aria-label={line.group}>
                        {#each line.marks as mark, m (m)}<Glyph image={mark.image} template={mark.template} size={10} tint={panel.dark ? 'rgb(230,230,230)' : 'rgb(46,46,46)'} />{/each}
                      </span>
                    {/if}
                  </span>
                {/if}
              </span>
            {/each}
          {/each}
        </span>
      {:else if gauge && usage !== null}
        <span class="gauge"><span class="gauge-fill" style:width="max(3px, {usage * 100}%)" style:background={barColor(entry.usageLevel, panel.dark)}
          style:--pen={barColor(entry.usageLevel, panel.dark)}></span></span>
      {/if}
      {#if entry.detail}<span class="detail">{entry.detail}</span>{/if}
    </span>
    {#if entry.children.length}
      <svg class="arrow" width="8" height="10" viewBox="0 0 8 10" aria-hidden="true"><path d="M2 1 6 5 2 9" fill="none" stroke="var(--label)" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" /></svg>
    {/if}
  </button>
{/if}

<style>
  .section {
    height: 25px;
    display: flex;
    align-items: center;
    font-size: 10.5px;
    font-weight: 500;
    color: var(--secondary);
  }
  .row {
    position: relative;
    display: flex;
    align-items: center;
    width: 100%;
    min-height: 21px;
    padding-top: 4px;
    padding-bottom: 4px;
    box-sizing: border-box;
    border-radius: 7px;
  }
  .row.tall { min-height: 27px; }
  .row.detailed { padding-top: 6px; padding-bottom: 6px; }
  .row:not(:disabled):hover { background: var(--hover); }
  .row:not(:disabled):active { background: var(--press); }
  .icon {
    position: absolute;
    top: 50%;
    margin-top: -7px;
  }
  .box {
    position: absolute;
    left: 13px;
    top: 50%;
    margin-top: -6px;
    width: 12px;
    height: 12px;
    box-sizing: border-box;
    border-radius: 3px;
    border: 1px solid color-mix(in srgb, var(--secondary) 50%, transparent);
  }
  .box.radio { border-radius: 6px; }
  .box.on { background: var(--accent); border-color: var(--accent); }
  .box svg { position: absolute; left: -1px; top: -1px; }
  .pip {
    position: absolute;
    left: 3px;
    top: 3px;
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: white;
  }
  .block {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }
  .title-line {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .light {
    position: absolute;
    top: 50%;
    margin-top: -3.5px;
  }
  .title {
    flex: 1;
    min-width: 24px;
    font-size: var(--title-size);
    line-height: 1.2;
    overflow-wrap: anywhere;
  }
  .value {
    flex: none;
    font-size: 11px;
    white-space: nowrap;
    line-height: 1.2;
  }
  .value.pill {
    padding: 2px 8px;
    border-radius: 7px;
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    color: var(--accent);
  }
  :global(:root[data-theme='dark']) .value.pill { background: color-mix(in srgb, var(--accent) 22%, transparent); color: rgb(140, 181, 240); }
  .value.configured {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--secondary);
  }
  .gauge {
    position: relative;
    height: 3px;
    margin-top: 4px;
    margin-bottom: 1px;
    border-radius: 1.5px;
    background: var(--track);
    overflow: hidden;
  }
  .pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    column-gap: 12px;
    row-gap: 5px;
    margin-top: 4px;
  }
  .half {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .pair .gauge { margin-top: 0; }
  .gauge-name {
    display: flex;
    gap: 3px;
    margin-top: 3px;
    font-size: 10px;
    line-height: 1.2;
    color: var(--secondary);
    white-space: nowrap;
  }
  .gauge-label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .gauge-name .num { flex: none; }
  /* At the right end of the line's names, under the end of its week's bar: whose models the line counts. */
  .marks {
    display: flex;
    align-items: center;
    gap: 3px;
    margin-left: auto;
    flex: none;
  }
  .gauge-fill {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    border-radius: 1.5px;
  }
  .detail {
    margin-top: 3px;
    font-size: 10px;
    color: var(--secondary);
    line-height: 1.2;
    overflow-wrap: anywhere;
    white-space: pre-line;
  }
  .arrow {
    position: absolute;
    right: 14px;
    top: 50%;
    margin-top: -5px;
  }
  .row.detailed .arrow, .row:has(.gauge) .arrow { top: calc(6px + 0.6 * var(--title-size)); }
  /* Beside the name and the first line of bars, as on a row with one bar, not down by the names under them. */
  .row:has(.pair) .icon { top: 15px; }
</style>
