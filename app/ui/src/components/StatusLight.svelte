<script lang="ts">
  // One status light for every row: new (accent), ok (green), issue (orange), anything else a hollow grey ring.
  let { state }: { state: string } = $props();
</script>

<span class="light {state}" aria-hidden="true"></span>

<style>
  .light {
    display: inline-block;
    flex: none;
    width: 7px;
    height: 7px;
    box-sizing: border-box;
    border-radius: 50%;
    border: 1px solid var(--secondary);
  }
  .new { background: var(--accent); border: 0; }
  .ok { background: var(--green); border: 0; }
  .issue { background: var(--orange); border: 0; }
</style>
