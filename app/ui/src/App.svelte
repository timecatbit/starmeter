<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';

  let version = $state('');
  invoke<string>('version').then((value) => (version = value));

  // Esc closes the panel and hands the focus back to the app used before it, as the 5.x panel did.
  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') invoke('close_panel');
  }
</script>

<svelte:window onkeydown={onKeydown} />

<main>
  <header>
    <h1>Summary</h1>
    <span class="version">Starmeter {version}</span>
  </header>
  <p class="note">Rust + Tauri 原型：玻璃面板、贴着星星弹出、点外面收起、Esc 交还焦点。</p>
</main>

<style>
  main {
    padding: 14px 16px;
  }
  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }
  h1 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }
  .version,
  .note {
    color: var(--secondary);
  }
  .note {
    margin: 10px 0 0;
    line-height: 1.5;
  }
</style>
