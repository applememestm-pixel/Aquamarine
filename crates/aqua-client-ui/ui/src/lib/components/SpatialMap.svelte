<script lang="ts">
  /** The voice room seen from above: you in the middle facing up; drag
   *  people to where you want to hear them. */
  import { t } from "../i18n";
  import { avatars } from "../people";
  import { spatial, spatialPos, placeOf, movePeer, resetPlaces, setRoomKind, type RoomKind } from "../spatial";

  let { users, me, speaking = {} }: { users: string[]; me: string; speaking?: Record<string, boolean> } = $props();
  let box: HTMLDivElement | undefined = $state();
  let drag = $state<string | null>(null);
  let order = $derived([...users].sort());
  let spots = $derived((void $spatialPos, order.map((u) => ({ u, ...placeOf(u, order) }))));

  function at(e: PointerEvent) {
    const r = box!.getBoundingClientRect();
    return { x: ((e.clientX - r.left) / r.width) * 2 - 1, y: ((e.clientY - r.top) / r.height) * 2 - 1 };
  }
  function down(e: PointerEvent, u: string) {
    drag = u;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function moveP(e: PointerEvent) {
    if (!drag || !box) return;
    const p = at(e);
    const d = Math.hypot(p.x, p.y);
    // Keep people out of your head and inside the room.
    const k = d < 0.18 ? 0.18 / Math.max(d, 0.001) : d > 0.95 ? 0.95 / d : 1;
    movePeer(drag, p.x * k, p.y * k);
  }
  const ROOMS: [RoomKind, string][] = [["none", "Open air"], ["room", "Small room"], ["hall", "Hall"]];
</script>

<div class="sm">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="sm-room" class:hall={$spatial.room === "hall"} class:small={$spatial.room === "room"} bind:this={box} onpointermove={moveP} onpointerup={() => (drag = null)}>
    <span class="sm-ring r1"></span><span class="sm-ring r2"></span>
    <span class="sm-me" title={$t("You")}>
      {#if $avatars[me]}<img src={$avatars[me]} alt="" />{:else}{me.charAt(0).toUpperCase()}{/if}
      <i class="sm-nose"></i>
    </span>
    {#each spots as s (s.u)}
      <button class="sm-p" class:speaking={speaking[s.u]} class:drag={drag === s.u} style:left="{(s.x + 1) * 50}%" style:top="{(s.y + 1) * 50}%" title={s.u}
        onpointerdown={(e) => down(e, s.u)}>
        {#if $avatars[s.u]}<img src={$avatars[s.u]} alt="" draggable="false" />{:else}{s.u.charAt(0).toUpperCase()}{/if}
        <small>{s.u}</small>
      </button>
    {/each}
    {#if !users.length}<span class="sm-empty">{$t("Nobody else here yet")}</span>{/if}
  </div>
  <div class="sm-side">
    <div class="sm-lbl">{$t("Acoustics")}</div>
    <div class="sm-seg">
      {#each ROOMS as [k, l] (k)}<button class:on={$spatial.room === k} onclick={() => setRoomKind(k)}>{$t(l)}</button>{/each}
    </div>
    <p class="sm-note">{$t("Drag people around you. Headphones make it work best.")}</p>
    <button class="sm-reset" onclick={resetPlaces}>{$t("Arrange in an arc")}</button>
  </div>
</div>

<style>
  .sm { display: flex; gap: 14px; align-items: center; padding: 10px 4px 2px; flex-wrap: wrap; justify-content: center; }
  .sm-room { position: relative; width: 210px; height: 210px; border-radius: 50%; flex-shrink: 0; touch-action: none;
    background: radial-gradient(circle, color-mix(in srgb, var(--accent) 10%, transparent), transparent 70%), var(--bg-input);
    border: 1px solid var(--border-strong); transition: box-shadow 0.3s; }
  .sm-room.small { box-shadow: inset 0 0 0 6px color-mix(in srgb, var(--accent) 10%, transparent); }
  .sm-room.hall { box-shadow: inset 0 0 0 14px color-mix(in srgb, var(--accent-2) 12%, transparent), 0 0 30px -8px var(--accent-glow); }
  .sm-ring { position: absolute; border-radius: 50%; border: 1px dashed var(--border-strong); pointer-events: none; }
  .r1 { inset: 30%; } .r2 { inset: 12%; opacity: 0.6; }
  .sm-me { position: absolute; left: 50%; top: 50%; width: 34px; height: 34px; margin: -17px 0 0 -17px; border-radius: 50%; display: grid; place-items: center;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070C; font-weight: 800; font-size: 13px; box-shadow: 0 0 0 3px var(--bg-shell); }
  .sm-me img { width: 100%; height: 100%; border-radius: 50%; object-fit: cover; }
  .sm-nose { position: absolute; top: -9px; left: 50%; margin-left: -5px; border-left: 5px solid transparent; border-right: 5px solid transparent; border-bottom: 7px solid var(--accent); }
  .sm-p { position: absolute; width: 32px; height: 32px; margin: -16px 0 0 -16px; padding: 0; border-radius: 50%; border: 2px solid var(--bg-shell); cursor: grab;
    display: grid; place-items: center; background: linear-gradient(135deg, #5b8cff, #a066ff); color: #fff; font: inherit; font-weight: 800; font-size: 12px;
    transition: box-shadow 0.15s, transform 0.12s; touch-action: none; }
  .sm-p img { width: 100%; height: 100%; border-radius: 50%; object-fit: cover; pointer-events: none; }
  .sm-p small { position: absolute; top: 100%; left: 50%; transform: translateX(-50%); margin-top: 2px; font-size: 10px; font-weight: 700; color: var(--fg-muted); white-space: nowrap; pointer-events: none; }
  .sm-p.speaking { box-shadow: 0 0 0 3px #3ddc84; }
  .sm-p.drag { cursor: grabbing; transform: scale(1.12); z-index: 2; }
  .sm-empty { position: absolute; left: 0; right: 0; bottom: 18%; text-align: center; font-size: 11.5px; color: var(--fg-dim); }
  .sm-side { display: flex; flex-direction: column; gap: 8px; min-width: 180px; max-width: 240px; }
  .sm-lbl { font-size: 11px; font-weight: 800; color: var(--fg-muted); text-transform: uppercase; letter-spacing: 0.05em; }
  .sm-seg { display: flex; gap: 3px; padding: 3px; border-radius: 10px; background: var(--bg-input); }
  .sm-seg button { flex: 1; border: none; background: none; color: var(--fg-muted); font: inherit; font-size: 12px; font-weight: 600; padding: 6px 4px; border-radius: 8px; cursor: pointer; white-space: nowrap; }
  .sm-seg button.on { background: var(--bg-shell); color: var(--accent); box-shadow: 0 0 0 1px var(--border-strong); }
  .sm-note { margin: 0; font-size: 11.5px; color: var(--fg-dim); line-height: 1.45; }
  .sm-reset { align-self: flex-start; border: 1px solid var(--border); background: none; color: var(--fg-muted); border-radius: 9px; padding: 6px 10px; font: inherit; font-size: 12px; font-weight: 600; cursor: pointer; }
  .sm-reset:hover { color: var(--accent); border-color: var(--accent); }
</style>
