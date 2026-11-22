<script lang="ts" module>
  /** empty: no tool on the homepage; loading: tools chosen, none read yet; ready: usage to show. */
  export type GuideState = 'empty' | 'loading' | 'ready';
</script>

<script lang="ts">
  // 新手指引: a first look at the panel, over the homepage itself. Each stop lights up the real thing (the figures, the
  // range, the cards, the plan and API rows, the header buttons) and dims the rest, with a word on what it is for; the
  // first stop welcomes and offers the language, so a reader of English never has to find the setting in Chinese first.
  // Shown once on the first run (the app keeps that it was seen); 通用 → 新手指引 shows it again.
  //
  // What it says follows what the homepage has. With no tool chosen yet (nothing found on this computer) there is no
  // usage to show, so after the welcome it says so and points to the gear, whose 主页模块 is where tools are chosen;
  // its last button opens that grid. While the first readings are still coming it says the figures will appear by
  // themselves. A stop whose part isn't on the homepage (no plan or API rows, Summary turned off) is left out.
  // Esc skips, Enter and the arrow keys move.
  import { onMount, tick } from 'svelte';
  import { chooseLanguage, language, NAMES, t, type Language } from '../lib/i18n.svelte';
  /** The tour's first card offers each language by its own name: 简体中文, 繁體中文, English. */
  const LANGUAGES: Language[] = ['zh', 'zh-Hant', 'en'];
  import { panel } from '../lib/ui.svelte';

  let { container, stage, rowKeys, onfinish, onmodules }: {
    container: HTMLElement; stage: GuideState;
    /** The modules of the plan and API rows: the rows stop lights these, not 工具更新 or Komari. */
    rowKeys: Set<string>;
    onfinish: () => void;
    /** Ends the tour and opens 主页模块, where tools are chosen. */
    onmodules: () => void;
  } = $props();

  interface Stop {
    title: string; body: string;
    /** What it lights up: every element these match, taken together (none: over the whole panel). The figures'
     *  labels and the heatmap bound the summary: the trend's own box reaches up behind the header. */
    targets: string[];
    /** At most this many of each selector's matches. */
    limit?: number;
    /** Only the matches this keeps. */
    keep?: (element: HTMLElement) => boolean;
    /** Shown even when nothing it names is on the homepage, over the whole panel. */
    always?: boolean;
    /** Shown only while this is on the homepage too (the figures, for words about them). */
    needs?: string;
    /** The last stop's own button in place of 完成. */
    action?: { label: string; run: () => void };
  }
  const SUMMARY = ['.summary .figure-label', '.heatmap'];
  const welcome: Stop = { title: '欢迎使用 Starmeter', targets: [],
    body: '它读取 AI 工具已经留在这台电脑上的用量记录，在一个面板里告诉你用了多少。不需要账号，任何内容都不会上传。花半分钟认识一下。' };
  function plan(stage: GuideState): Stop[] {
    if (stage === 'empty') return [welcome,
      // Only the figures: the panel is short with nothing in it, and the words go under them.
      { title: '还没有用量', targets: ['.summary .figure-label', '.summary .figure'], always: true,
        body: '这台电脑上还没找到 AI 编程工具的用量记录。Starmeter 读的是 Claude Code、Codex、Cursor、Gemini 等工具自己留在本机的记录：装好其中一个、用过一次，这里就会出现数字。' },
      { title: '选择要读的工具', targets: ['.summary .buttons > .slot:nth-child(2)'], always: true,
        body: '点齿轮打开“主页模块”，勾选你在用的工具；工具的记录不在默认位置时，点它的名字选文件夹。“通用”里的“自动检测”会把这台电脑上找得到的都勾上。',
        action: { label: '打开主页模块', run: () => onmodules() } },
    ];
    return [welcome,
      { title: '用量总览', targets: SUMMARY, needs: '.summary .figure',
        body: stage === 'loading'
          ? '最上面是这段时间的 API 等值估算、Token 和请求数，下面是趋势和热力图。第一次读取要一点时间，读到以后数字会自己出现。'
          : '最上面是这段时间的 API 等值估算、Token 和请求数，下面是趋势和热力图。估算按官方价格计算，不是你的账单。' },
      { title: '统计范围', targets: ['.summary .picker'], body: '从今天到全部共 8 种范围，整页跟着切换。' },
      { title: '每个工具一张卡', targets: ['.cards .tabs', '.cards .card', '.cards .no-calls'], limit: 1,
        body: '卡片列出每个工具用的模型和用量，点开是它的详情页。也可以按模型、项目分组；“会话”里是正在运行的 Claude Code 和 Codex 会话。' },
      { title: '订阅额度和 API 余额', targets: ['.rows .row'], limit: 3, keep: (e) => rowKeys.has(e.dataset.key ?? ''),
        body: '带箭头的行点开是详情：每个额度窗口用了多少、何时重置，账户余额，以及这一项的设置。' },
      { title: '三个按钮', targets: ['.summary .buttons > .slot'],
        body: '图钉：钉住后面板浮在最上层并实时刷新，可以拖动、调大小。齿轮：选择主页显示哪些模块，设置目录、Key、语言和外观。分享：把用量做成一张图。' },
    ];
  }

  /** The stops for the homepage as it is: worked out when the tour starts and again if tools come or go. */
  let stops = $state<Stop[]>([welcome]);
  let index = $state(0);
  const stop = $derived(stops[Math.min(index, stops.length - 1)]);
  const last = $derived(index >= stops.length - 1);
  let hole = $state<{ left: number; top: number; width: number; height: number } | null>(null);
  let bubbleHeight = $state(0);
  let width = $state(360), height = $state(600);
  let primary: HTMLButtonElement | undefined = $state();

  function matches(s: Stop) {
    const found = s.targets.flatMap((selector) => [...container.querySelectorAll<HTMLElement>(selector)]
      .filter((e) => (e.offsetWidth > 0 || e.offsetHeight > 0) && (!s.keep || s.keep(e))).slice(0, s.limit ?? Infinity));
    return found;
  }
  const shown = (s: Stop) => (!s.needs || !!container.querySelector(s.needs)) && (!s.targets.length || s.always || matches(s).length > 0);

  /** The lit area: everything the stop names, together, a little roomier than its parts, inside the panel. */
  function measure() {
    const box = container.getBoundingClientRect();
    width = box.width;
    height = box.height;
    const found = matches(stop);
    if (!found.length) { hole = null; return; }
    const rects = found.map((e) => e.getBoundingClientRect());
    const left = Math.max(4, Math.min(...rects.map((r) => r.left)) - box.left - 5);
    const top = Math.max(4, Math.min(...rects.map((r) => r.top)) - box.top - 5);
    const right = Math.min(box.width - 4, Math.max(...rects.map((r) => r.right)) - box.left + 5);
    const bottom = Math.min(box.height - 4, Math.max(...rects.map((r) => r.bottom)) - box.top + 5);
    hole = { left, top, width: Math.max(0, right - left), height: Math.max(0, bottom - top) };
  }

  /** The words sit under the lit area when they fit there, else above it; with nothing lit, in the middle. */
  const bubbleWidth = $derived(Math.min(width - 24, 340));
  const bubbleLeft = $derived((width - bubbleWidth) / 2);
  const placement = $derived.by(() => {
    if (!hole) return { top: Math.max(12, (height - bubbleHeight) / 2), above: false };
    const below = hole.top + hole.height + 12;
    if (below + bubbleHeight <= height - 8) return { top: below, above: false };
    const above = hole.top - 12 - bubbleHeight;
    if (above >= 8) return { top: above, above: true };
    return { top: Math.max(8, height - bubbleHeight - 8), above: false };
  });
  /** The little arrow points at the middle of the lit area, kept within the bubble's rounded corners. */
  const arrowX = $derived(hole ? Math.min(bubbleWidth - 22, Math.max(22, hole.left + hole.width / 2 - bubbleLeft)) : 0);

  async function show(next: number) {
    index = Math.max(0, Math.min(next, stops.length - 1));
    await tick();
    // Brought into view first (the rows may be below the fold); the lit area follows the scroll each frame.
    matches(stops[index])[0]?.scrollIntoView({ block: 'nearest', behavior: panel.reduceMotion ? 'auto' : 'smooth' });
    primary?.focus({ preventScroll: true });
  }
  const next = () => (last ? (stop.action ? stop.action.run() : finish()) : void show(index + 1));
  const previous = () => { if (index > 0) void show(index - 1); };
  function finish() { onfinish(); }

  // The stops follow the homepage: worked out as the tour starts and for a moment after tools come or go (the first
  // readings, or one chosen meanwhile), while the homepage draws its new parts. The stop on screen keeps its place
  // when it is still among them; one that left gives way to the stop now in its place.
  let replanUntil = 0;
  function replan() {
    const title = stop.title, next = plan(stage).filter(shown);
    if (next.length === stops.length && next.every((s, i) => s.title === stops[i].title && s.body === stops[i].body)) return;
    stops = next;
    const at = next.findIndex((s) => s.title === title);
    index = at >= 0 ? at : Math.min(index, next.length - 1);
  }
  let planned: GuideState | null = null;
  $effect(() => {
    const now = stage;
    if (planned !== now) { planned = now; replanUntil = performance.now() + 1500; }
  });

  onMount(() => {
    stops = plan(stage).filter(shown);
    replanUntil = performance.now() + 1000;
    let frame = 0;
    const follow = () => {
      if (performance.now() < replanUntil) replan();
      measure();
      frame = requestAnimationFrame(follow);
    };
    follow();
    // Ahead of the panel's own keys: Esc here skips the tour instead of going back or closing the panel.
    const keys = (event: KeyboardEvent) => {
      const handled = event.key === 'Escape' || event.key === 'ArrowRight' || event.key === 'ArrowLeft' || (event.key === 'Enter' && event.target === document.body);
      if (!handled) return;
      event.preventDefault();
      event.stopPropagation();
      if (event.key === 'Escape') finish();
      else if (event.key === 'ArrowLeft') previous();
      else next();
    };
    window.addEventListener('keydown', keys, true);
    void show(0);
    return () => { cancelAnimationFrame(frame); window.removeEventListener('keydown', keys, true); };
  });
</script>

<div class="guide" class:still={panel.reduceMotion} role="dialog" aria-modal="true" aria-labelledby="guide-title" data-stage={stage}
  style:--dim={panel.dark ? 'rgba(0, 0, 0, 0.55)' : 'rgba(8, 10, 24, 0.42)'}>
  {#if hole}
    <div class="hole" style:left="{hole.left}px" style:top="{hole.top}px" style:width="{hole.width}px" style:height="{hole.height}px"></div>
  {:else}
    <div class="dim"></div>
  {/if}
  <div class="bubble" class:above={placement.above} bind:offsetHeight={bubbleHeight}
    style:left="{bubbleLeft}px" style:top="{placement.top}px" style:width="{bubbleWidth}px">
    {#if hole}<span class="arrow" class:up={!placement.above} style:left="{arrowX}px"></span>{/if}
    {#key index}
      <div class="words">
        {#if index > 0}<p class="count">{index} / {stops.length - 1}</p>{/if}
        <h2 id="guide-title">{t(stop.title)}</h2>
        <p class="body">{t(stop.body)}</p>
        {#if index === 0}
          <div class="language" role="radiogroup" aria-label={t('界面语言')}>
            <span class="pill" style:transform="translateX({LANGUAGES.indexOf(language.current) * 100}%)"></span>
            {#each LANGUAGES as key (key)}
              <button class="choice pressable" class:chosen={language.current === key} role="radio" aria-checked={language.current === key}
                onclick={() => chooseLanguage(key)}>{NAMES[key]}</button>
            {/each}
          </div>
        {/if}
        {#if last}<p class="later">{t('以后可以在 齿轮 → 通用 → 新手指引 再看一遍。')}</p>{/if}
      </div>
    {/key}
    <div class="actions">
      {#if !last}<button class="skip pressable" onclick={finish}>{t('跳过')}</button>{/if}
      <span class="spacer"></span>
      {#if index > 0}<button class="step pressable" onclick={previous}>{t('上一步')}</button>{/if}
      {#if last && stop.action}<button class="step pressable" onclick={finish}>{t('稍后')}</button>{/if}
      <button class="step primary pressable" bind:this={primary} onclick={next}>
        {t(last ? stop.action?.label ?? '完成' : index === 0 ? '开始' : '下一步')}
      </button>
    </div>
  </div>
</div>

<style>
  .guide {
    position: absolute;
    inset: 0;
    z-index: 40;
    border-radius: inherit;
    overflow: hidden;
  }
  .dim { position: absolute; inset: 0; background: var(--dim); }
  /* The lit area: a clear rounded window in a dim that covers the rest of the panel. */
  .hole {
    position: absolute;
    border-radius: 10px;
    box-shadow: 0 0 0 2000px var(--dim);
    transition: left 0.32s var(--page-curve), top 0.32s var(--page-curve), width 0.32s var(--page-curve), height 0.32s var(--page-curve);
  }
  .bubble {
    position: absolute;
    box-sizing: border-box;
    padding: 14px 14px 12px;
    border-radius: 12px;
    background: var(--solid);
    color: var(--label);
    box-shadow: 0 0 0 0.5px var(--separator), 0 10px 30px rgba(0, 0, 0, 0.22);
    transition: top 0.32s var(--page-curve);
  }
  .still .hole, .still .bubble { transition: none; }
  .arrow {
    position: absolute;
    width: 10px;
    height: 10px;
    margin-left: -5px;
    background: var(--solid);
    transform: rotate(45deg);
    bottom: -5px;
    box-shadow: 0.5px 0.5px 0 0 var(--separator);
    transition: left 0.32s var(--page-curve);
  }
  .arrow.up { top: -5px; bottom: auto; box-shadow: -0.5px -0.5px 0 0 var(--separator); }
  .words { animation: words 0.24s ease-out; }
  .still .words { animation: none; }
  @keyframes words { from { opacity: 0; transform: translateY(3px); } }
  .count { margin: 0 0 3px; font-size: 10.5px; color: var(--secondary); font-variant-numeric: tabular-nums; }
  h2 { margin: 0; font-size: 14px; font-weight: 600; line-height: 1.3; }
  .body { margin: 6px 0 0; font-size: 12px; line-height: 1.5; color: var(--label); opacity: 0.85; }
  .later { margin: 8px 0 0; font-size: 11px; line-height: 1.45; color: var(--secondary); }
  .language {
    position: relative;
    display: flex;
    height: 28px;
    margin-top: 12px;
    padding: 2px;
    box-sizing: border-box;
    border-radius: 8px;
    background: color-mix(in srgb, var(--label) 6%, transparent);
  }
  .pill {
    position: absolute;
    top: 2px;
    bottom: 2px;
    left: 2px;
    width: calc((100% - 4px) / 3);
    border-radius: 6px;
    background: var(--solid);
    box-shadow: 0 0 0 0.5px var(--separator), 0 1px 3px rgba(0, 0, 0, 0.1);
    transition: transform 0.22s var(--page-curve);
  }
  .still .pill { transition: none; }
  .choice {
    --press-scale: 0.96;
    position: relative;
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 12px;
    color: var(--secondary);
    border-radius: 6px;
  }
  .choice.chosen { color: var(--label); font-weight: 600; }
  .actions { display: flex; align-items: center; gap: 6px; margin-top: 14px; }
  .spacer { flex: 1; }
  .skip {
    --press-scale: 0.96;
    height: 26px;
    padding: 0 6px;
    border-radius: 7px;
    font-size: 12px;
    color: var(--secondary);
  }
  .skip:hover { background: var(--hover); }
  .step {
    --press-scale: 0.96;
    height: 28px;
    padding: 0 14px;
    border-radius: 8px;
    font-size: 12px;
    font-weight: 600;
    color: var(--label);
    background: color-mix(in srgb, var(--label) 8%, transparent);
  }
  .step:hover { background: color-mix(in srgb, var(--label) 12%, transparent); }
  .step.primary { color: var(--on-accent); background: var(--accent); }
  .step.primary:hover { background: color-mix(in srgb, var(--accent) 88%, black); }
</style>
