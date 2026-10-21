<script lang="ts">
  /** Mount a heavy, rarely used component only once it's first needed
   *  (settings, editors…): its code is fetched and compiled on demand
   *  instead of at start-up, and it stays mounted afterwards. */
  import type { Component } from "svelte";
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let { load, when }: { load: () => Promise<{ default: Component<any> }>; when: boolean } = $props();
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let C = $state<Component<any> | null>(null);
  let asked = false;
  $effect(() => {
    if (when && !asked) {
      asked = true;
      void load().then((m) => (C = m.default)).catch((e) => { asked = false; console.error("lazy load", e); });
    }
  });
</script>

{#if C}<C />{/if}
