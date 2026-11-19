<script lang="ts">
  // A question answered in place, under the row that asked: a line of text (Return saves, Esc cancels), one of two
  // choices, or a confirmation with what it will do; a notice says what went wrong. No window pops up over the panel.
  import { onDestroy, onMount } from 'svelte';
  import { answer, ask } from '../lib/ask.svelte';
  import { pageCurve } from '../lib/motion';
  import { panel } from '../lib/ui.svelte';
  import type { Question } from '../lib/types';

  let { question, indent = 0, sheet = false }: { question: Question; indent?: number; sheet?: boolean } = $props();

  let field: HTMLInputElement | undefined = $state();
  // svelte-ignore state_referenced_locally
  let text = $state(question.value ?? '');
  const destructive = $derived(/删除|清除|移除|覆盖|关闭|停止|重置/.test(question.accept ?? ''));

  onMount(() => {
    ask.placed++;
    if (field) { field.focus(); field.select(); }
  });
  onDestroy(() => { ask.placed--; });

  function keydown(event: KeyboardEvent) {
    // The panel's own Esc (back) and Return stay out of it while the question is open.
    event.stopPropagation();
    if (event.key === 'Escape') { event.preventDefault(); answer(null); }
    if (event.key === 'Enter' && question.kind === 'text' && !event.isComposing) { event.preventDefault(); answer(text); }
  }

  const grow = (_node: Element) => ({
    duration: panel.reduceMotion ? 0 : 220, easing: pageCurve,
    css: (t: number) => `opacity: ${t}; transform: translateY(${-4 * (1 - t)}px)`,
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions (Return and Esc answer from the field or either button) -->
<div class="ask" class:sheet class:notice={question.kind === 'notice'} style:margin-left="{indent}px" role="group"
  aria-label={question.title} onkeydown={keydown} in:grow>
  <!-- Under its row a field's title only repeats the row's name; what to type is the line that matters. -->
  {#if sheet || question.kind !== 'text'}<p class="title">{question.title}</p>{/if}
  {#if question.detail}<p class="detail" class:lead={!sheet && question.kind === 'text'}>{question.detail}</p>{/if}
  {#if question.kind === 'text'}
    <input bind:this={field} class="field" type={question.hidden ? 'password' : 'text'} bind:value={text} autocomplete="off" spellcheck="false"
      aria-label={question.title} />
  {/if}
  <div class="buttons">
    {#if question.kind === 'notice'}
      <button class="button pressable" onclick={() => answer(null)}>好</button>
    {:else}
      <button class="button pressable" onclick={() => answer(question.kind === 'confirm' ? false : null)}>取消</button>
      {#if question.kind === 'confirm'}
        <button class="button primary pressable" class:danger={destructive} onclick={() => answer(true)}>{question.accept || '确定'}</button>
      {:else if question.kind === 'text'}
        <button class="button primary pressable" onclick={() => answer(text)}>保存</button>
      {:else if question.kind === 'choice'}
        {#each question.options ?? [] as [label, value] (value)}
          <button class="button primary pressable" onclick={() => answer(value)}>{label}</button>
        {/each}
      {/if}
    {/if}
  </div>
</div>

<style>
  .ask {
    margin: 2px 12px 8px;
    padding: 10px 12px;
    border-radius: 10px;
    background: color-mix(in srgb, var(--label) 5%, transparent);
    box-shadow: inset 0 0 0 0.5px var(--separator);
  }
  .ask.sheet {
    margin: 0;
    background: var(--solid);
    box-shadow: 0 0 0 0.5px var(--separator), 0 -6px 24px rgba(0, 0, 0, 0.16);
  }
  .ask.notice { background: color-mix(in srgb, var(--orange) 9%, transparent); }
  .title { margin: 0; font-size: 12px; font-weight: 600; line-height: 1.35; }
  .detail { margin: 4px 0 0; font-size: 11px; line-height: 1.45; color: var(--secondary); white-space: pre-line; }
  .detail.lead { margin-top: 0; }
  .field {
    display: block;
    width: 100%;
    box-sizing: border-box;
    margin-top: 8px;
    height: 28px;
    padding: 0 8px;
    border-radius: 7px;
    border: 1px solid var(--separator);
    background: var(--solid);
    color: var(--label);
    font: inherit;
    font-size: 12px;
    outline: none;
  }
  .field:focus { border-color: var(--accent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent); }
  .buttons { display: flex; justify-content: flex-end; flex-wrap: wrap; gap: 6px; margin-top: 10px; }
  .button {
    --press-scale: 0.95;
    height: 26px;
    padding: 0 12px;
    border-radius: 7px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--label);
    background: color-mix(in srgb, var(--label) 8%, transparent);
  }
  .button:hover { background: color-mix(in srgb, var(--label) 12%, transparent); }
  .button.primary { color: var(--on-accent); background: var(--accent); }
  .button.primary:hover { background: color-mix(in srgb, var(--accent) 88%, black); }
  .button.danger { color: white; background: var(--red); }
  .button.danger:hover { background: color-mix(in srgb, var(--red) 88%, black); }
</style>
