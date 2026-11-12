<script lang="ts">
  /** Telegram's "QR code" screen: the code on a white card over a gradient
   *  with an emoji doodle pattern, the avatar sitting on the card's top
   *  edge, the @name under the code in the theme's colours, a row of themes
   *  to pick from (remembered) and "Share". */
  import QrCode from "./QrCode.svelte";
  import { t, tr } from "../i18n";
  import { showToast } from "./Toast.svelte";
  import { onBack } from "../back";

  let { text, username, avatar = "", letter = "", onclose, hint = "" }:
    { text: string; username: string; avatar?: string; letter?: string; onclose: () => void; hint?: string } = $props();

  const THEMES: { id: string; a: string; b: string; c: string; e: string }[] = [
    { id: "sea", a: "#2AABEE", b: "#7B5CFF", c: "#1E3A8A", e: "🐳" },
    { id: "mint", a: "#00C9A7", b: "#3D8BFD", c: "#0F4C5C", e: "🌿" },
    { id: "rose", a: "#FF6B9A", b: "#B05CFF", c: "#5B1A4A", e: "🌸" },
    { id: "sun", a: "#FFB347", b: "#FF5E62", c: "#6B2A12", e: "🌞" },
    { id: "night", a: "#5B6CFF", b: "#1FD1F9", c: "#0B1230", e: "🌙" },
    { id: "lime", a: "#7ED957", b: "#00B894", c: "#173B1F", e: "🍀" },
    { id: "ink", a: "#30343F", b: "#6C7A89", c: "#0D0F14", e: "⭐" },
  ];
  const KEY = "aqua.qrTheme.v1";
  let themeId = $state("sea");
  try { const v = localStorage.getItem(KEY); if (v && THEMES.some((x) => x.id === v)) themeId = v; } catch { /* none */ }
  const th = $derived(THEMES.find((x) => x.id === themeId) ?? THEMES[0]);
  function pick(id: string) { themeId = id; try { localStorage.setItem(KEY, id); } catch { /* none */ } }

  /** The doodle: the theme's emoji scattered and turned, tiled. */
  function pattern(e: string): string {
    const spots: [number, number, number, number][] = [[18, 22, -18, 22], [70, 14, 14, 16], [118, 40, -6, 20], [40, 74, 24, 18], [96, 92, -22, 24], [14, 120, 8, 16], [64, 134, -12, 20], [126, 128, 20, 16]];
    const g = spots.map(([x, y, a, s]) => `<text x='${x}' y='${y}' font-size='${s}' transform='rotate(${a} ${x} ${y})' text-anchor='middle' dominant-baseline='middle'>${e}</text>`).join("");
    return `url("data:image/svg+xml;utf8,${encodeURIComponent(`<svg xmlns='http://www.w3.org/2000/svg' width='144' height='150'>${g}</svg>`)}")`;
  }

  async function share() {
    try {
      const nav = navigator as Navigator & { share?: (d: { text: string; title?: string }) => Promise<void> };
      if (nav.share) { await nav.share({ title: "@" + username, text }); return; }
    } catch { /* cancelled */ }
    try { await navigator.clipboard?.writeText(text); showToast(tr("Copied")); } catch { /* none */ }
  }
  $effect(() => onBack(onclose));
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="qs" style:--a={th.a} style:--b={th.b} style:--c={th.c} onclick={(e) => { if (e.target === e.currentTarget) onclose(); }}>
  <div class="qs-pat" style:background-image={pattern(th.e)}></div>
  <button class="qs-x" data-x onclick={onclose} aria-label={$t("Close")}>
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>
  </button>
  <div class="qs-card">
    <div class="qs-ava">{#if avatar}<img src={avatar} alt="" />{:else}<span>{letter}</span>{/if}</div>
    {#key themeId}<div class="qs-code"><QrCode {text} color={th.a} color2={th.b} tg size={236} /></div>{/key}
    <div class="qs-name">@{username.toUpperCase()}</div>
  </div>
  {#if hint}<div class="qs-hint">{hint}</div>{/if}
  <div class="qs-bottom">
    <div class="qs-themes" role="radiogroup" aria-label={$t("Theme")}>
      {#each THEMES as x (x.id)}
        <button class="qs-th" class:on={x.id === themeId} role="radio" aria-checked={x.id === themeId} aria-label={x.e} onclick={() => pick(x.id)}
          style:background={`linear-gradient(135deg, ${x.a}, ${x.b})`}><span>{x.e}</span></button>
      {/each}
    </div>
    <button class="qs-share" onclick={share}>{$t("Share QR code")}</button>
  </div>
</div>

<style>
  .qs { position: fixed; inset: 0; z-index: 160; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 18px;
    background: radial-gradient(120% 80% at 20% 0%, var(--a), transparent 70%), radial-gradient(120% 90% at 100% 100%, var(--b), transparent 70%), var(--c);
    animation: qs-in .28s cubic-bezier(.2,.9,.25,1); padding: calc(64px + var(--sat, 0px)) 16px calc(16px + var(--sab, 0px)); box-sizing: border-box; transition: background .4s; }
  @keyframes qs-in { from { opacity: 0; } }
  .qs-pat { position: absolute; inset: 0; opacity: .22; pointer-events: none; background-size: 144px 150px; }
  .qs-x { position: absolute; top: calc(12px + var(--sat, 0px)); left: 12px; width: 42px; height: 42px; border-radius: 50%; border: none; display: grid; place-items: center;
    background: rgba(0,0,0,.25); color: #fff; cursor: pointer; }
  .qs-x svg { width: 20px; height: 20px; }
  .qs-card { position: relative; margin-top: 40px; padding: 58px 22px 18px; border-radius: calc(28px * var(--rk, 1)); background: #fff; display: flex; flex-direction: column; align-items: center;
    box-shadow: 0 30px 60px -24px rgba(0,0,0,.55); animation: qs-card .38s cubic-bezier(.2,1.1,.3,1); }
  @keyframes qs-card { from { transform: translateY(24px) scale(.94); opacity: 0; } }
  .qs-ava { position: absolute; top: -46px; left: 50%; transform: translateX(-50%); width: 92px; height: 92px; border-radius: 50%; overflow: hidden; display: grid; place-items: center;
    background: linear-gradient(135deg, var(--a), var(--b)); border: 5px solid #fff; box-shadow: 0 8px 22px -8px rgba(0,0,0,.4); }
  .qs-ava img { width: 100%; height: 100%; object-fit: cover; }
  .qs-ava span { color: #fff; font: 800 34px var(--font-ui, inherit); }
  .qs-code { animation: qs-fade .3s ease; }
  @keyframes qs-fade { from { opacity: .3; } }
  .qs-name { margin-top: 10px; font: 900 22px var(--font-display, var(--font-ui, inherit)); letter-spacing: .02em;
    background: linear-gradient(135deg, var(--a), var(--b)); -webkit-background-clip: text; background-clip: text; color: transparent; }
  .qs-hint { position: relative; max-width: 300px; text-align: center; color: rgba(255,255,255,.85); font-size: 13px; }
  .qs-bottom { position: relative; width: min(420px, 100%); margin-top: auto; padding: 14px; border-radius: calc(22px * var(--rk, 1)); display: flex; flex-direction: column; gap: 12px;
    background: rgba(10, 12, 18, .55); -webkit-backdrop-filter: blur(16px); backdrop-filter: blur(16px); }
  .qs-themes { display: flex; gap: 10px; overflow-x: auto; scrollbar-width: none; padding: 3px; }
  .qs-th { flex: 0 0 auto; width: 52px; height: 64px; border-radius: calc(14px * var(--rk, 1)); border: 2px solid transparent; cursor: pointer; display: grid; place-items: center; padding: 0; transition: transform .15s; }
  .qs-th span { font-size: 22px; }
  .qs-th.on { border-color: #fff; transform: scale(1.04); }
  .qs-share { height: 48px; border: none; border-radius: calc(14px * var(--rk, 1)); background: linear-gradient(135deg, var(--a), var(--b)); color: #fff; font: 700 15.5px var(--font-ui, inherit); cursor: pointer; }
  .qs-share:active { filter: brightness(.92); }
  @media (min-width: 721px) { .qs-bottom { margin-top: 0; } }
</style>
