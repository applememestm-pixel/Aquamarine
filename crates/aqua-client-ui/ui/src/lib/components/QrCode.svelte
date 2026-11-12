<script lang="ts">
  /** A QR code with an avatar (or letter) in the middle and rounded eyes,
   *  coloured like the profile card — Telegram style. Square modules and
   *  level "Q": round dots and a bigger hole didn't scan reliably. */
  import { qrMatrix } from "../qr";

  let { text, color = "#00e5c7", color2 = "", avatar = "", letter = "", size = 220, plain = false, tg = false, bg = "#fff" }:
    { text: string; color?: string; color2?: string; avatar?: string; letter?: string; size?: number; plain?: boolean;
      /** Telegram's look: modules joined into rounded blobs, a gradient ink. */ tg?: boolean; bg?: string } = $props();
  const gid = "qg" + Math.random().toString(36).slice(2, 8);
  /** One module as a path: corners round only where it has no neighbour,
   *  so runs of modules join into smooth blobs (Telegram) — the module's
   *  area stays almost whole, which keeps the code readable. */
  function blob(r: number, c: number): string {
    const d = (rr: number, cc: number) => rr >= 0 && cc >= 0 && rr < n && cc < n && m[rr][cc] && !finder(rr, cc) && !inHole(rr, cc);
    const up = d(r - 1, c), dn = d(r + 1, c), lf = d(r, c - 1), rt = d(r, c + 1);
    const R = 0.5;
    const tl = !up && !lf ? R : 0, tr = !up && !rt ? R : 0, br = !dn && !rt ? R : 0, bl = !dn && !lf ? R : 0;
    const x = c, y = r;
    return `M${x + tl} ${y}H${x + 1 - tr}${tr ? `A${tr} ${tr} 0 0 1 ${x + 1} ${y + tr}` : ""}V${y + 1 - br}${br ? `A${br} ${br} 0 0 1 ${x + 1 - br} ${y + 1}` : ""}H${x + bl}${bl ? `A${bl} ${bl} 0 0 1 ${x} ${y + 1 - bl}` : ""}V${y + tl}${tl ? `A${tl} ${tl} 0 0 1 ${x + tl} ${y}` : ""}Z`;
  }

  // `plain` (long codes — meet nearby): black on white, nothing in the
  // middle, the lowest error correction — the fewest, biggest squares.
  const m = $derived(qrMatrix(text, plain ? "L" : tg ? "M" : "Q"));
  const n = $derived(m.length);
  const ink = $derived(plain ? "#0b0e14" : tg && color2 ? `url(#${gid})` : color);
  const tgPath = $derived.by(() => {
    if (!tg) return "";
    let d = "";
    for (let r = 0; r < n; r++) for (let c = 0; c < n; c++) if (m[r][c] && !finder(r, c) && !inHole(r, c)) d += blob(r, c);
    return d;
  });
  /** White margin: the standard 4 squares for camera scans of long codes. */
  const qz = $derived(plain ? 4 : 2);
  // Space in the middle for the avatar (kept small: "M" survives ~15 %).
  const hole = $derived(plain || tg ? 0 : Math.floor(n * 0.2) | 1);
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
    {#if tg && color2}
      <defs><linearGradient id={gid} x1="0" y1="0" x2="1" y2="1" gradientUnits="objectBoundingBox"><stop offset="0" stop-color={color} /><stop offset="1" stop-color={color2} /></linearGradient></defs>
    {/if}
    <rect x={-qz} y={-qz} width={n + 2 * qz} height={n + 2 * qz} rx="3" fill={bg} />
    {#if tg}
      <path d={tgPath} fill={ink} />
    {:else}
    {#each m as row, r (r)}
      {#each row as dark, c (c)}
        {#if dark && !finder(r, c) && !inHole(r, c)}
          <rect x={c - 0.01} y={r - 0.01} width="1.02" height="1.02" fill={ink} />
        {/if}
      {/each}
    {/each}
    {/if}
    {#each [[0, 0], [0, n - 7], [n - 7, 0]] as [r, c] (r + "," + c)}
      <rect x={c + 0.5} y={r + 0.5} width="6" height="6" rx={tg ? 2.2 : 1.8} fill="none" stroke={ink} stroke-width="1" />
      <rect x={c + 2} y={r + 2} width="3" height="3" rx={tg ? 1.2 : 0.9} fill={ink} />
    {/each}
  </svg>
  {#if !plain && !tg}
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
