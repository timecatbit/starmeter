<script lang="ts">
  // A row's or a card's mark: the engine's image (a one-colour template takes the text colour) or a named symbol.
  import Icon from './Icon.svelte';

  let { image, template = false, symbol, size = 14, tint = 'var(--label)' }:
    { image?: string; template?: boolean; symbol?: string; size?: number; tint?: string } = $props();

  const source = $derived.by(() => {
    if (!image) return null;
    const type = image.startsWith('iVBOR') ? 'image/png' : image.startsWith('/9j/') ? 'image/jpeg' : 'image/svg+xml';
    return `data:${type};base64,${image}`;
  });
</script>

{#if source && template}
  <span class="mask" style:width="{size}px" style:height="{size}px" style:background-color={tint}
    style:mask-image="url('{source}')" style:-webkit-mask-image="url('{source}')"></span>
{:else if source}
  <img src={source} alt="" width={size} height={size} draggable="false" />
{:else if symbol}
  <span class="symbol" style:color={tint}><Icon name={symbol} {size} /></span>
{/if}

<style>
  .mask {
    display: block;
    flex: none;
    mask-size: contain;
    -webkit-mask-size: contain;
    mask-repeat: no-repeat;
    -webkit-mask-repeat: no-repeat;
    mask-position: center;
    -webkit-mask-position: center;
  }
  img {
    display: block;
    flex: none;
    object-fit: contain;
  }
  .symbol {
    display: block;
    flex: none;
  }
</style>
