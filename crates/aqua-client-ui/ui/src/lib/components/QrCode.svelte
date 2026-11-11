<script lang="ts">
  /** A QR code with an avatar (or letter) in the middle and rounded eyes,
   *  coloured like the profile card — Telegram style. Square modules and
   *  level "Q": round dots and a bigger hole didn't scan reliably. */
  import { qrMatrix } from "../qr";

  let { text, color = "#00e5c7", avatar = "", letter = "", size = 220, plain = false }:
    { text: string; color?: string; avatar?: string; letter?: string; size?: number; plain?: boolean } = $props();

  // `plain` (long codes — meet nearby): black on white, nothing in the
  // middle, the lowest error correction — the fewest, biggest squares.
  const m = $derived(qrMatrix(text, plain ? "L" : "Q"));
  const n = $derived(m.length);
  const ink = $derived(plain ? "#0b0e14" : color);
  /** White margin: the standard 4 squares for camera scans of long codes. */
  const qz = $derived(plain ? 4 : 2);
  // Space in the middle for the avatar (kept small: "M" survives ~15 %).
  const hole = $derived(plain ? 0 : Math.floor(n * 0.2) | 1);
  const h0 = $derived(Math.floor((n - hole) / 2));
  function finder(r: number, c: number): boolean {
    return (r < 7 && c < 7) || (r < 7 && c >= n - 7) || (r >= n - 7 && c < 7);
  }
  function inHole(r: number, c: number): boolean {
    return r >= h0 && r < h0 + hole && c >= h0 && c < h0 + hole;
  }
</script>

<div class="qr" style:width="{size}px" style:height="{size}px" style:--qc={color}>
  <svg viewBox="{-qz} {-qz} {n + 2 * qz} {n + 2 * qz}" width={size} height={size} aria-label="QR">
    <rect x={-qz} y={-qz} width={n + 2 * qz} height={n + 2 * qz} rx="3" fill="#fff" />
    {#each m as row, r (r)}
      {#each row as dark, c (c)}
        {#if dark && !finder(r, c) && !inHole(r, c)}
          <rect x={c - 0.01} y={r - 0.01} width="1.02" height="1.02" fill={ink} />
        {/if}
      {/each}
    {/each}
    {#each [[0, 0], [0, n - 7], [n - 7, 0]] as [r, c] (r + "," + c)}
      <rect x={c + 0.5} y={r + 0.5} width="6" height="6" rx="1.8" fill="none" stroke={ink} stroke-width="1" />
      <rect x={c + 2} y={r + 2} width="3" height="3" rx="0.9" fill={ink} />
    {/each}
  </svg>
  {#if !plain}
  <div class="mid" style:width="{(hole / (n + 4)) * size}px" style:height="{(hole / (n + 4)) * size}px">
    {#if avatar}<img src={avatar} alt="" />{:else}<span>{letter}</span>{/if}
  </div>
  {/if}
</div>

<style>
  .qr { position: relative; display: grid; place-items: center; }
  svg { display: block; border-radius: calc(18px * var(--rk, 1)); }
  .mid { position: absolute; border-radius: 50%; overflow: hidden; background: var(--qc); display: grid; place-items: center;
    box-shadow: 0 0 0 4px #fff; }
  .mid img { width: 100%; height: 100%; object-fit: cover; }
  .mid span { color: #fff; font-weight: 800; font-size: 20px; }
</style>
