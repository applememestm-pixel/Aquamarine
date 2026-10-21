<script module lang="ts">
  import { writable } from "svelte/store";
  /**
   * Telegram mini apps (bot web apps), the way Telegram Web runs them: the
   * page goes in an iframe with the launch parameters in its #hash, and the
   * official telegram-web-app.js talks to us with postMessage
   * ({eventType, eventData} as JSON). Works the same on every platform.
   */
  export interface MiniAppOpen {
    bot: string;
    url: string;
    /** Button that opened it (sendData reports it as button_text). */
    buttonText?: string;
    /** Where it was opened from: keyboard buttons may sendData. */
    from: "inline" | "reply" | "menu" | "link";
    startParam?: string;
  }
  export const miniApp = writable<MiniAppOpen | null>(null);
  export function openMiniApp(o: MiniAppOpen) { miniApp.set(o); }
</script>

<script lang="ts">
  import { t, tr } from "../i18n";
  import { api } from "../tauri";
  import { showToast } from "./Toast.svelte";
  import { askConfirm } from "./Confirm.svelte";
  import { get } from "svelte/store";

  interface MainBtn { visible: boolean; active: boolean; text: string; color?: string; textColor?: string; progress: boolean }
  const NO_BTN: MainBtn = { visible: false, active: true, text: "", progress: false };

  let frame = $state<HTMLIFrameElement | null>(null);
  let src = $state("");
  let loading = $state(true);
  let error = $state<string | null>(null);
  let signed = $state(true);
  let main = $state<MainBtn>({ ...NO_BTN });
  let second = $state<MainBtn>({ ...NO_BTN });
  let back = $state(false);
  let settingsBtn = $state(false);
  let headerColor = $state<string | null>(null);
  let bgColor = $state<string | null>(null);
  let confirmClose = $state(false);
  let fullscreen = $state(false);
  let popup = $state<{ title: string; message: string; buttons: { id: string; type: string; text: string }[] } | null>(null);
  let title = $state("");

  /** Hex of a CSS colour (computed through the browser). */
  function hex(css: string): string {
    const el = document.createElement("span");
    el.style.color = css;
    document.body.appendChild(el);
    const c = getComputedStyle(el).color;
    el.remove();
    const m = c.match(/\d+(\.\d+)?/g);
    if (!m) return "#000000";
    return "#" + m.slice(0, 3).map((x) => Math.round(+x).toString(16).padStart(2, "0")).join("");
  }

  function themeParams() {
    const v = (n: string) => hex(getComputedStyle(document.documentElement).getPropertyValue(n).trim() || "#000");
    const bg = v("--bg-shell"), sec = v("--bg-list"), fg = v("--fg"), dim = v("--fg-dim"), acc = v("--accent");
    return {
      bg_color: bg, secondary_bg_color: sec, text_color: fg, hint_color: dim, link_color: acc,
      button_color: acc, button_text_color: "#06070c", header_bg_color: sec, bottom_bar_bg_color: sec,
      accent_text_color: acc, section_bg_color: v("--bg-input"), section_header_text_color: acc,
      section_separator_color: hex("rgba(127,127,127,0.2)"), subtitle_text_color: v("--fg-muted"), destructive_text_color: "#ff5c5c",
    };
  }

  function post(eventType: string, eventData: unknown = {}) {
    try { frame?.contentWindow?.postMessage(JSON.stringify({ eventType, eventData }), "*"); } catch { /* ignore */ }
  }

  function viewport() {
    const h = frame?.clientHeight ?? 600;
    post("viewport_changed", { height: h, width: frame?.clientWidth ?? 400, is_expanded: true, is_state_stable: true });
  }

  $effect(() => {
    const o = $miniApp;
    // Reset per app.
    main = { ...NO_BTN }; second = { ...NO_BTN }; back = false; settingsBtn = false; headerColor = null; bgColor = null;
    confirmClose = false; popup = null; error = null; loading = true; src = ""; fullscreen = false;
    if (!o) return;
    try { title = new URL(o.url).host; } catch { title = o.bot; }
    void launch(o);
  });

  async function launch(o: MiniAppOpen) {
    let init = "";
    try {
      const r = await api.webappInit(o.bot, o.startParam);
      init = r.init_data; signed = r.signed;
    } catch (e) {
      signed = false;
      console.warn("webapp init", e);
    }
    if ($miniApp !== o) return;
    let u: URL;
    try { u = new URL(o.url); } catch { error = tr("This mini app has a broken address."); return; }
    if (u.protocol !== "https:" && u.protocol !== "http:") { error = tr("This mini app has a broken address."); return; }
    const launchParams = new URLSearchParams({
      tgWebAppData: init,
      tgWebAppVersion: "8.0",
      tgWebAppPlatform: /android/i.test(navigator.userAgent) ? "android" : "tdesktop",
      tgWebAppThemeParams: JSON.stringify(themeParams()),
      ...(o.startParam ? { tgWebAppStartParam: o.startParam } : {}),
    }).toString();
    u.hash = u.hash ? `${u.hash.slice(1)}&${launchParams}` : launchParams;
    src = u.toString();
  }

  async function close(force = false) {
    if (confirmClose && !force) {
      const ok = await askConfirm({ title: tr("Close the mini app?"), text: tr("Changes you made may not be saved."), ok: tr("Close"), danger: true });
      if (!ok) return;
    }
    miniApp.set(null);
  }

  function cloudKey(bot: string) { return `aqua.cloud.${bot}`; }
  function cloud(bot: string): Record<string, string> {
    try { return JSON.parse(localStorage.getItem(cloudKey(bot)) ?? "{}"); } catch { return {}; }
  }
  function saveCloud(bot: string, c: Record<string, string>) {
    try { localStorage.setItem(cloudKey(bot), JSON.stringify(c)); } catch { /* ignore */ }
  }

  function customMethod(o: MiniAppOpen, req_id: string, method: string, params: Record<string, unknown>) {
    const c = cloud(o.bot);
    let result: unknown = true, err: string | undefined;
    const keys = (params.keys as string[] | undefined) ?? [];
    switch (method) {
      case "saveStorageValue":
        if (Object.keys(c).length >= 1024 && !(String(params.key) in c)) err = "STORAGE_KEYS_TOO_MANY";
        else { c[String(params.key)] = String(params.value ?? "").slice(0, 4096); saveCloud(o.bot, c); }
        break;
      case "getStorageValues": result = Object.fromEntries(keys.map((k) => [k, c[k] ?? ""])); break;
      case "deleteStorageValues": for (const k of keys) delete c[k]; saveCloud(o.bot, c); break;
      case "getStorageKeys": result = Object.keys(c); break;
      default: err = "UNKNOWN_METHOD";
    }
    post("custom_method_invoked", err ? { req_id, error: err } : { req_id, result });
  }

  function btnFrom(d: Record<string, unknown>): MainBtn {
    return {
      visible: !!d.is_visible && !!String(d.text ?? "").trim(),
      active: d.is_active !== false,
      text: String(d.text ?? ""),
      color: d.color ? String(d.color) : undefined,
      textColor: d.text_color ? String(d.text_color) : undefined,
      progress: !!d.is_progress_visible,
    };
  }

  async function onMessage(e: MessageEvent) {
    const o = $miniApp;
    if (!o || !frame || e.source !== frame.contentWindow) return;
    let msg: { eventType?: string; eventData?: unknown };
    try { msg = typeof e.data === "string" ? JSON.parse(e.data) : e.data; } catch { return; }
    const type = msg?.eventType;
    let d = (msg?.eventData ?? {}) as Record<string, unknown>;
    if (typeof msg?.eventData === "string") { try { d = JSON.parse(msg.eventData); } catch { d = {}; } }
    switch (type) {
      case "iframe_ready":
      case "web_app_request_theme":
        post("theme_changed", { theme_params: themeParams() });
        if (type === "iframe_ready") viewport();
        break;
      case "web_app_ready": loading = false; break;
      case "web_app_request_viewport":
      case "web_app_expand": viewport(); break;
      case "web_app_request_safe_area": post("safe_area_changed", { top: 0, bottom: 0, left: 0, right: 0 }); break;
      case "web_app_request_content_safe_area": post("content_safe_area_changed", { top: 0, bottom: 0, left: 0, right: 0 }); break;
      case "web_app_close": void close(true); break;
      case "web_app_setup_main_button": main = btnFrom(d); break;
      case "web_app_setup_secondary_button": second = btnFrom(d); break;
      case "web_app_setup_back_button": back = !!d.is_visible; break;
      case "web_app_setup_settings_button": settingsBtn = !!d.is_visible; break;
      case "web_app_setup_closing_behavior": confirmClose = !!d.need_confirmation; break;
      case "web_app_set_header_color": headerColor = d.color ? String(d.color) : null; break;
      case "web_app_set_background_color": bgColor = d.color ? String(d.color) : null; break;
      case "web_app_request_fullscreen": fullscreen = true; post("fullscreen_changed", { is_fullscreen: true }); setTimeout(viewport, 50); break;
      case "web_app_exit_fullscreen": fullscreen = false; post("fullscreen_changed", { is_fullscreen: false }); setTimeout(viewport, 50); break;
      case "web_app_open_link": {
        const url = String(d.url ?? "");
        if (/^https?:\/\//i.test(url)) void api.openExternal(url).catch(() => {});
        break;
      }
      case "web_app_open_tg_link": {
        const path = String(d.path_full ?? "");
        const m = /^\/([a-z0-9_.]{3,32})(?:\?start=([\w-]{1,64}))?/i.exec(path);
        if (m) {
          void close(true);
          const name = m[1].toLowerCase(), start = m[2];
          void import("../stores").then(async (st) => {
            if (get(st.chats).some((c) => c.id === name)) await st.selectChat(name);
            else await st.startChat(name, start ? `/start ${start}` : "/start");
          }).catch(() => {});
        }
        break;
      }
      case "web_app_open_popup": {
        const btns = (Array.isArray(d.buttons) ? d.buttons : [{ id: "", type: "close" }]) as { id?: string; type?: string; text?: string }[];
        popup = {
          title: String(d.title ?? ""), message: String(d.message ?? ""),
          buttons: btns.slice(0, 3).map((b) => ({ id: String(b.id ?? ""), type: String(b.type ?? "default"), text: String(b.text ?? "") })),
        };
        break;
      }
      case "web_app_read_text_from_clipboard": {
        let text: string | null = null;
        try { text = await navigator.clipboard.readText(); } catch { text = null; }
        post("clipboard_text_received", { req_id: d.req_id, data: text });
        break;
      }
      case "web_app_request_write_access": post("write_access_requested", { status: "allowed" }); break;
      case "web_app_request_phone": post("phone_requested", { status: "cancelled" }); break;
      case "web_app_biometry_get_info": post("biometry_info_received", { available: false }); break;
      case "web_app_open_scan_qr_popup": post("scan_qr_popup_closed", {}); break;
      case "web_app_invoke_custom_method": customMethod(o, String(d.req_id ?? ""), String(d.method ?? ""), (d.params ?? {}) as Record<string, unknown>); break;
      case "web_app_data_send": {
        if (o.from !== "reply") { showToast(tr("This mini app can't send data from here")); break; }
        const data = String(d.data ?? "").slice(0, 4096);
        try {
          await api.botSendRaw(o.bot, "[wad:v1]" + JSON.stringify({ t: o.buttonText ?? "", d: data }));
          void close(true);
        } catch (err) { showToast(String(err)); }
        break;
      }
      default: break; // haptics, header colours we don't need, …
    }
  }

  function popupPick(id: string) {
    popup = null;
    post("popup_closed", { button_id: id });
  }
</script>

<svelte:window onmessage={onMessage} onresize={viewport} onkeydown={(e) => { if ($miniApp && e.key === "Escape" && !popup) { e.preventDefault(); if (back) post("back_button_pressed"); else void close(); } }} />

{#if $miniApp}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="ma-veil" onclick={(e) => { if (e.target === e.currentTarget) void close(); }}>
    <div class="ma" class:full={fullscreen} role="dialog" aria-modal="true" aria-label={$t("Mini app")} style:background={bgColor ?? undefined}>
      <header class="ma-head" style:background={headerColor ?? undefined}>
        <button class="ma-ic ma-close" onclick={() => close()} aria-label={$t("Close")}>✕</button>
        {#if back}
          <button class="ma-ic ma-back" onclick={() => post("back_button_pressed")} aria-label={$t("Back")}>←</button>
        {/if}
        <div class="ma-title">
          <b>@{$miniApp.bot}</b>
          <span>{title}{#if !signed} · {$t("not verified")}{/if}</span>
        </div>
        {#if settingsBtn}<button class="ma-ic" onclick={() => post("settings_button_pressed")} aria-label={$t("Settings")}>⚙</button>{/if}
        <button class="ma-ic" onclick={() => { fullscreen = !fullscreen; post("fullscreen_changed", { is_fullscreen: fullscreen }); setTimeout(viewport, 50); }} aria-label={$t("Full screen")}>{fullscreen ? "▣" : "⛶"}</button>
        <button class="ma-ic" onclick={() => { if (frame && src) { loading = true; const s = src; src = ""; queueMicrotask(() => (src = s)); } }} aria-label={$t("Reload")}>↻</button>
      </header>
      <div class="ma-body">
        {#if error}
          <div class="ma-err">{error}</div>
        {:else if src}
          <iframe
            bind:this={frame}
            {src}
            title={$miniApp.bot}
            sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-modals allow-downloads allow-popups-to-escape-sandbox"
            allow="clipboard-read; clipboard-write; camera; microphone; geolocation; fullscreen"
            onload={() => { setTimeout(() => { loading = false; viewport(); }, 600); }}
          ></iframe>
        {/if}
        {#if loading && !error}<div class="ma-load"><span class="ma-spin"></span></div>{/if}
        {#if popup}
          <div class="ma-pop-veil">
            <div class="ma-pop" role="alertdialog">
              {#if popup.title}<b>{popup.title}</b>{/if}
              <p>{popup.message}</p>
              <div class="ma-pop-btns">
                {#each popup.buttons as b, i (i)}
                  <button class:danger={b.type === "destructive"} onclick={() => popupPick(b.id)}>
                    {b.text || (b.type === "ok" ? "OK" : b.type === "cancel" ? $t("Cancel") : $t("Close"))}
                  </button>
                {/each}
              </div>
            </div>
          </div>
        {/if}
      </div>
      {#if main.visible || second.visible}
        <footer class="ma-foot">
          {#if second.visible}
            <button class="ma-btn second" disabled={!second.active} onclick={() => post("secondary_button_pressed")}
              style:background={second.color} style:color={second.textColor}>{second.progress ? "…" : second.text}</button>
          {/if}
          {#if main.visible}
            <button class="ma-btn" disabled={!main.active} onclick={() => post("main_button_pressed")}
              style:background={main.color} style:color={main.textColor}>
              {#if main.progress}<span class="ma-spin sm"></span>{:else}{main.text}{/if}
            </button>
          {/if}
        </footer>
      {/if}
    </div>
  </div>
{/if}

<style>
  .ma-veil { position: fixed; inset: 0; z-index: 950; display: grid; place-items: center; background: rgba(4, 6, 12, 0.55); animation: ma-fade 0.15s ease-out; }
  .ma {
    width: min(440px, calc(100vw - 16px)); height: min(760px, calc(100dvh - 24px));
    display: flex; flex-direction: column; overflow: hidden;
    background: var(--bg-shell); border: 1px solid var(--border-strong); border-radius: 18px;
    animation: ma-pop 0.2s cubic-bezier(.2,.9,.3,1.15);
  }
  .ma.full { width: 100vw; height: 100dvh; border-radius: 0; border: none; }
  .ma-head { display: flex; align-items: center; gap: 6px; padding: 8px 8px 8px 6px; background: var(--bg-list); border-bottom: 1px solid var(--border); }
  .ma-ic { width: 32px; height: 32px; border-radius: 50%; border: none; background: transparent; color: var(--fg-muted); font-size: 16px; cursor: pointer; display: grid; place-items: center; }
  .ma-ic:hover { background: var(--bg-hover); color: var(--fg); }
  .ma-title { flex: 1; min-width: 0; display: flex; flex-direction: column; line-height: 1.2; }
  .ma-title b { font-size: 14px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ma-title span { font-size: 11.5px; color: var(--fg-dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ma-body { position: relative; flex: 1; min-height: 0; }
  .ma-body iframe { width: 100%; height: 100%; border: 0; display: block; background: transparent; }
  .ma-load { position: absolute; inset: 0; display: grid; place-items: center; background: var(--bg-shell); }
  .ma-spin { width: 28px; height: 28px; border-radius: 50%; border: 3px solid var(--accent-soft); border-top-color: var(--accent); animation: ma-rot 0.8s linear infinite; display: inline-block; }
  .ma-spin.sm { width: 16px; height: 16px; border-width: 2px; border-color: rgba(0,0,0,0.2); border-top-color: currentColor; }
  .ma-err { padding: 30px 20px; color: var(--fg-muted); text-align: center; }
  .ma-foot { display: flex; gap: 8px; padding: 10px; background: var(--bg-list); border-top: 1px solid var(--border); }
  .ma-btn { flex: 1; height: 44px; border: none; border-radius: 12px; font: inherit; font-weight: 700; font-size: 15px; cursor: pointer; background: var(--accent); color: #06070C; display: grid; place-items: center; }
  .ma-btn.second { background: var(--bg-input); color: var(--accent); }
  .ma-btn:disabled { opacity: 0.55; cursor: default; }
  .ma-pop-veil { position: absolute; inset: 0; display: grid; place-items: center; background: rgba(0,0,0,0.45); padding: 20px; }
  .ma-pop { width: min(320px, 100%); background: var(--bg-shell); border: 1px solid var(--border-strong); border-radius: 14px; padding: 16px; display: flex; flex-direction: column; gap: 8px; }
  .ma-pop p { margin: 0; color: var(--fg-muted); font-size: 14px; line-height: 1.45; white-space: pre-wrap; }
  .ma-pop-btns { display: flex; justify-content: flex-end; gap: 6px; margin-top: 6px; }
  .ma-pop-btns button { border: none; background: none; color: var(--accent); font: inherit; font-weight: 700; padding: 8px 10px; border-radius: 8px; cursor: pointer; }
  .ma-pop-btns button:hover { background: var(--bg-hover); }
  .ma-pop-btns button.danger { color: #ff5c5c; }
  @keyframes ma-fade { from { opacity: 0; } }
  @keyframes ma-pop { from { opacity: 0; transform: translateY(14px) scale(0.97); } }
  @keyframes ma-rot { to { transform: rotate(360deg); } }
</style>
