<script lang="ts">
  import { iconSvg } from '../lib/icons';

  let { name, size = 14 }: { name: string; size?: number } = $props();
</script>

<svg class="icon" viewBox="0 0 20 20" width={size} height={size} aria-hidden="true">{@html iconSvg(name)}</svg>

<style>
  .icon {
    display: block;
    flex: none;
  }
</style>
