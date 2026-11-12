<script lang="ts">
  /**
   * Telegram-style chat themes: a row of ready themes, then every part on its
   * own — accent, background, my bubbles, their bubbles — each in a section
   * that folds open. Changes show at once (in the chat behind and the preview).
   */
  import { PRESETS, chatThemes, setChatTheme as setLocal, themeStyle, resolve, sharedThemes, setThemeShared, setGroupThemeShared, themeChanged, type ChatTheme } from "../chatTheme";
  import { groupMap } from "../groups";
  import { info } from "../stores";
  import { can } from "../groupRules";
  import { PALETTES, WALLPAPERS } from "../theme";
  import ColorInput from "./ColorInput.svelte";
  import { t, tr } from "../i18n";

  let { chat, name = "", onclose }: { chat: string; name?: string; onclose: () => void } = $props();

  // Every change here also goes to the other person when "for both of us" is on.
  const setChatTheme = (id: string, th: ChatTheme | null) => { setLocal(id, th); themeChanged(id); };
  const oneToOne = $derived(!!chat && !chat.startsWith("grp_") && !chat.startsWith("__"));
  const grp = $derived(chat.startsWith("grp_") ? $groupMap[chat] : undefined);
  /** Groups and channels: admins who may change the info set it for everyone. */
  const groupAdmin = $derived(!!grp && can(grp, $info?.username ?? "", "change_info"));
  const cur = $derived<ChatTheme>($chatThemes[chat] ?? {});
  const look = $derived(resolve(cur));
  const set = (patch: Partial<ChatTheme>) => setChatTheme(chat, { ...cur, ...patch });
  const pick = (preset: string | null) => setChatTheme(chat, preset ? { preset } : null);
  const WALLS = WALLPAPERS.filter((w) => w.css);
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="ct-back" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }}>
  <div class="ct" role="dialog" aria-modal="true" aria-label={$t("Chat theme")} tabindex="-1"
    onkeydown={(e) => { if (e.key === "Escape") { e.stopPropagation(); onclose(); } }}>
    <header><h3>{$t("Chat theme")}</h3>{#if name}<span class="sub">{name}</span>{/if}<button data-x class="x" onclick={onclose} aria-label={$t("Close")}>×</button></header>

    <!-- A little chat in the chosen look -->
    <div class="pv" style={themeStyle(cur)}>
      <div class="pv-wall" style:background={look?.wall ?? "var(--chat-wallpaper, var(--bg-shell))"}></div>
      <div class="b in" style:background={look?.in ?? "var(--bg-bubble-in)"} style:color={look?.in ? "var(--bubble-in-fg)" : "var(--fg)"}>{$t("Hi! How's it going?")}</div>
      <div class="b out" style:background={look?.out ?? "color-mix(in srgb, var(--accent) 22%, var(--bg-bubble-in))"} style:color={look?.out ? "var(--bubble-out-fg)" : "var(--fg)"}>{$t("Great — look at this theme 🎨")}</div>
    </div>

    {#if oneToOne}
      <!-- Telegram: a chat theme is for both people. -->
      <label class="both">
        <input type="checkbox" checked={$sharedThemes.has(chat)} onchange={(e) => setThemeShared(chat, (e.currentTarget as HTMLInputElement).checked)} />
        <span><b>{$t("For both of us")}</b><small>{name ? tr("{name} sees the same theme in this chat", { name }) : $t("The other person sees the same theme in this chat")}</small></span>
      </label>
    {:else if groupAdmin}
      <label class="both">
        <input type="checkbox" checked={$sharedThemes.has(chat)} onchange={(e) => setGroupThemeShared(chat, (e.currentTarget as HTMLInputElement).checked)} />
        <span><b>{grp?.kind === "channel" ? $t("For all subscribers") : $t("For all members")}</b><small>{$t("Everyone sees this theme, including those who join later")}</small></span>
      </label>
    {:else if grp?.theme_at}
      <div class="both"><span><small>{$t("The admins set this chat's theme for everyone; your changes stay on this device until they change it again.")}</small></span></div>
    {/if}
    <div class="presets">
      <button class="pr" class:on={!look} onclick={() => pick(null)}><span class="pr-sw none">✕</span><small>{$t("No theme")}</small></button>
      {#each PRESETS as p (p.id)}
        <button class="pr" class:on={cur.preset === p.id && !cur.accent && !cur.wall && !cur.out && !cur.in} onclick={() => pick(p.id)} title={$t(p.name)}>
          <span class="pr-sw" style:background={p.wall}><i style:background={p.out}></i><i style:background={p.in}></i><em>{p.emoji}</em></span>
          <small>{$t(p.name)}</small>
        </button>
      {/each}
    </div>

    <details class="sec" open>
      <summary>{$t("Accent")}<span class="dot" style:background={look?.accent ?? "var(--accent)"}></span></summary>
      <div class="sw">
        {#each PALETTES as p (p.id)}<button class="c" class:on={cur.accent === p.accent} style:background={p.accent} onclick={() => set({ accent: p.accent })} aria-label={p.name}></button>{/each}
        <ColorInput value={look?.accent ?? "#00e5c7"} size={30} oninput={(v) => set({ accent: v })} />
        {#if cur.accent}<button class="lnk" onclick={() => set({ accent: undefined })}>{$t("As in the theme")}</button>{/if}
      </div>
    </details>
    <details class="sec">
      <summary>{$t("Background")}<span class="dot" style:background={look?.wall ?? "var(--chat-wallpaper, var(--bg-shell))"}></span></summary>
      <div class="walls">
        {#each WALLS as w (w.id)}<button class="w" class:on={cur.wall === w.css} style:background={w.css} onclick={() => set({ wall: w.css })} title={$t(w.name)}></button>{/each}
      </div>
      <div class="sw"><span class="lbl">{$t("Plain colour")}</span><ColorInput value="#101522" size={30} oninput={(v) => set({ wall: v })} />
        {#if cur.wall}<button class="lnk" onclick={() => set({ wall: undefined })}>{$t("As in the theme")}</button>{/if}</div>
    </details>
    <details class="sec">
      <summary>{$t("My messages")}<span class="dot" style:background={look?.out ?? "var(--accent-soft)"}></span></summary>
      <div class="sw"><ColorInput value={look?.out ?? "#1E5A84"} size={30} oninput={(v) => set({ out: v })} />
        {#if cur.out}<button class="lnk" onclick={() => set({ out: undefined })}>{$t("As in the theme")}</button>{/if}</div>
    </details>
    <details class="sec">
      <summary>{$t("Their messages")}<span class="dot" style:background={look?.in ?? "var(--bg-bubble-in)"}></span></summary>
      <div class="sw"><ColorInput value={look?.in ?? "#14283A"} size={30} oninput={(v) => set({ in: v })} />
        {#if cur.in}<button class="lnk" onclick={() => set({ in: undefined })}>{$t("As in the theme")}</button>{/if}</div>
    </details>

    <p class="note">{$t("Only on this device. Text colour adjusts itself so it always reads.")}</p>
    <div class="row"><button class="btn" onclick={() => pick(null)}>{$t("Reset")}</button><span class="grow"></span><button class="btn ok" onclick={onclose}>{$t("Done")}</button></div>
  </div>
</div>

<style>
  .both { display: flex; align-items: flex-start; gap: 10px; margin: 2px 0 12px; padding: 10px 12px; border-radius: calc(12px * var(--rk, 1)); background: var(--bg-input); cursor: pointer; }
  .both input { margin-top: 3px; accent-color: var(--accent); width: 18px; height: 18px; }
  .both span { display: flex; flex-direction: column; gap: 2px; }
  .both b { font-size: 14px; }
  .both small { font-size: 12px; color: var(--fg-dim); }
  .ct-back { position: fixed; inset: 0; z-index: 10080; display: grid; place-items: center; background: rgba(0, 0, 0, .45); }
  :global(html.phone) .ct-back { place-items: end stretch; }
  .ct { width: min(460px, 94vw); max-height: 88vh; overflow-y: auto; display: flex; flex-direction: column; gap: 12px; padding: 18px;
    background: var(--bg-surface-2, var(--bg-list)); color: var(--fg); border: 1px solid var(--border-strong); border-radius: calc(16px * var(--rk, 1));
    box-shadow: 0 30px 70px -20px rgba(0, 0, 0, .7); animation: ct-in .2s cubic-bezier(.16, 1, .3, 1); }
  :global(html.phone) .ct { width: 100%; border-radius: calc(18px * var(--rk, 1)) calc(18px * var(--rk, 1)) 0 0; padding-bottom: calc(18px + var(--sab, env(safe-area-inset-bottom, 0px))); }
  @keyframes ct-in { from { opacity: 0; transform: translateY(16px); } }
  header { display: flex; align-items: baseline; gap: 8px; }
  h3 { margin: 0; font-size: 17px; }
  .sub { color: var(--fg-dim); font-size: 13px; flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .x { margin-left: auto; border: none; background: var(--bg-input); color: var(--fg); width: 28px; height: 28px; border-radius: 50%; cursor: pointer; font-size: 16px; }
  .pv { position: relative; display: flex; flex-direction: column; gap: 6px; padding: 14px; border-radius: calc(12px * var(--rk, 1)); overflow: hidden; border: 1px solid var(--border); }
  .pv-wall { position: absolute; inset: 0; z-index: 0; }
  .b { position: relative; z-index: 1; max-width: 75%; padding: 7px 11px; border-radius: 14px; font-size: 13.5px; }
  .b.in { align-self: flex-start; border-top-left-radius: 4px; }
  .b.out { align-self: flex-end; border-top-right-radius: 4px; }
  .presets { display: grid; grid-template-columns: repeat(auto-fill, minmax(70px, 1fr)); gap: 8px; }
  .pr { display: flex; flex-direction: column; align-items: center; gap: 4px; border: 2px solid transparent; background: none; color: var(--fg-muted); padding: 4px; border-radius: calc(12px * var(--rk, 1)); cursor: pointer; font: inherit; }
  .pr.on { border-color: var(--accent); color: var(--fg); }
  .pr-sw { position: relative; width: 100%; aspect-ratio: 1; border-radius: calc(10px * var(--rk, 1)); overflow: hidden; display: block; border: 1px solid var(--border); }
  .pr-sw i { position: absolute; height: 18%; border-radius: 6px; }
  .pr-sw i:first-child { right: 10%; top: 22%; width: 52%; }
  .pr-sw i:nth-child(2) { left: 10%; top: 48%; width: 46%; }
  .pr-sw em { position: absolute; left: 6px; bottom: 4px; font-style: normal; font-size: 18px; }
  .pr-sw.none { display: grid; place-items: center; background: var(--bg-input); color: var(--fg-dim); font-size: 18px; }
  .pr small { font-size: 11.5px; }
  .sec { border: 1px solid var(--border); border-radius: calc(12px * var(--rk, 1)); background: var(--bg-input); }
  .sec summary { display: flex; align-items: center; gap: 10px; padding: 11px 12px; cursor: pointer; list-style: none; font-weight: 600; font-size: 14px; }
  .sec summary::-webkit-details-marker { display: none; }
  .sec summary::after { content: "›"; margin-left: 8px; color: var(--fg-dim); transition: transform .15s; }
  .sec[open] summary::after { transform: rotate(90deg); }
  .dot { margin-left: auto; width: 18px; height: 18px; border-radius: 50%; border: 1px solid var(--border-strong); }
  .sw { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 0 12px 12px; }
  .c { width: 30px; height: 30px; border-radius: 50%; border: 2px solid transparent; cursor: pointer; padding: 0; }
  .c.on { border-color: var(--fg); }
  .walls { display: grid; grid-template-columns: repeat(auto-fill, minmax(56px, 1fr)); gap: 6px; padding: 0 12px 10px; }
  .w { aspect-ratio: 3 / 4; border-radius: calc(8px * var(--rk, 1)); border: 2px solid var(--border); cursor: pointer; padding: 0; }
  .w.on { border-color: var(--accent); }
  .lbl { font-size: 13px; color: var(--fg-muted); }
  .lnk { border: none; background: none; color: var(--accent); font: inherit; font-size: 13px; cursor: pointer; padding: 0; }
  .note { margin: 0; font-size: 12px; color: var(--fg-dim); }
  .row { display: flex; gap: 8px; align-items: center; }
  .grow { flex: 1; }
  .btn { border: 1px solid var(--border-strong); background: var(--bg-input); color: var(--fg); font: inherit; font-weight: 600; padding: 8px 14px; border-radius: calc(10px * var(--rk, 1)); cursor: pointer; }
  .btn.ok { background: var(--accent); border-color: var(--accent); color: #06231f; }
</style>
