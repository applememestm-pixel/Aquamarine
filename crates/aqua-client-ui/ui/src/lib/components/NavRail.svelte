<script lang="ts">
  import { appIconUrl } from "../appIcon";
  import { t } from "../i18n";
  import { icons } from "../icons";
  import { info, activeTab, type NavTab } from "../stores";
  import { openSettings } from "./SettingsPanel.svelte";
</script>

<aside class="rail">
  <img class="brand" src={$appIconUrl} alt={$t("Aquamarine")} title={$t("Aquamarine")} draggable="false" />
  <div class="rail-divider"></div>

  {#each [
    { id: "chats",    label: "Chats",    svg: icons.chats },
    { id: "calls",    label: "Calls",    svg: icons.phone },
    { id: "files",    label: "Files",    svg: icons.files },
    { id: "contacts", label: "Contacts", svg: icons.contacts },
  ] as item (item.id)}
    <button
      class="nav-btn"
      class:active={$activeTab === item.id}
      data-label={item.label}
      onclick={() => activeTab.set(item.id as NavTab)}
    >{@html item.svg}<span class="nav-lbl">{item.label}</span></button>
  {/each}

  <div class="rail-spacer"></div>

  <!-- Compact connection pill — green when connected, dim when not.
       No button: connection is handled automatically. On first launch
       the registration/connect wizard pops up by itself. -->
  <div
    class="status-pill"
    class:connected={$info?.connected}
    title={$info?.connected ? `Connected to ${$info?.server_url ?? "server"} as @${$info?.username ?? "?"}` : "Not connected — auto-retrying…"}
  >
    <span class="pill-dot"></span>
  </div>

  <!-- Settings / profile live in the user panel on desktop; the rail keeps
       a settings button only in the narrow (mobile) layout. -->
  <button class="nav-btn mobile-only" data-label="Settings" onclick={() => openSettings()}>
    {@html icons.settings}<span class="nav-lbl">{$t("Settings")}</span>
  </button>
</aside>

<style>
  .mobile-only { display: none; }
  @media (max-width: 720px) { .mobile-only { display: grid; } }
  .rail {
    background: var(--bg-rail);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 14px 0;
    gap: 4px;
  }
  .brand {
    width: 40px; height: 40px;
    border-radius: 12px;
    display: block; object-fit: cover;
    margin-bottom: 10px;
    box-shadow: 0 0 24px -4px var(--accent-glow);
    transition: transform .35s cubic-bezier(.2,.8,.2,1), box-shadow .35s;
    letter-spacing: -0.02em;
  }
  .rail-divider {
    width: 32px; height: 1px;
    background: var(--border);
    margin: 4px 0 10px;
  }
  .nav-btn {
    width: 44px; height: 44px;
    display: grid; place-items: center;
    background: transparent;
    border: none;
    border-radius: 12px;
    color: var(--fg-muted);
    cursor: pointer;
    transition: all 0.15s ease;
    position: relative;
  }
  .nav-btn::before {
    content: '';
    position: absolute;
    left: -14px; top: 50%;
    width: 3px; height: 0;
    background: var(--accent);
    border-radius: 0 3px 3px 0;
    transform: translateY(-50%);
    transition: height 0.2s ease;
    box-shadow: 0 0 8px var(--accent-glow);
  }
  .nav-btn:hover { background: var(--bg-hover); color: var(--fg); }
  .nav-btn.active {
    color: var(--accent);
    background: var(--accent-soft);
  }
  .nav-btn.active::before { height: 22px; }
  .status-pill {
    display: grid; place-items: center;
    width: 36px; height: 14px;
    margin: 2px 0 6px;
  }
  .pill-dot {
    width: 8px; height: 8px;
    border-radius: 50%;
    background: var(--fg-dim);
    box-shadow: none;
    transition: background 0.3s ease, box-shadow 0.3s ease;
    animation: pill-fade 1.6s ease-in-out infinite;
  }
  .status-pill.connected .pill-dot {
    background: var(--online);
    box-shadow: 0 0 8px var(--online);
    animation: none;
  }
  @keyframes pill-fade {
    0%, 100% { opacity: 0.9; }
    50%      { opacity: 0.3; }
  }
  .nav-btn :global(svg) { width: 20px; height: 20px; }
  .rail-spacer { flex: 1; }
  .me {
    width: 40px; height: 40px;
    border-radius: 50%;
    background: linear-gradient(135deg, var(--accent) 0%, var(--accent-2) 100%);
    display: grid; place-items: center;
    font-weight: 700; color: #06070C;
    cursor: pointer;
    position: relative;
    box-shadow: 0 4px 12px -4px var(--accent-glow);
    border: none;
    font-family: inherit;
    font-size: 15px;
    letter-spacing: -0.01em;
    transition: transform 0.15s ease;
  }
  .me:hover { transform: scale(1.05); }
  .me::after {
    content: '';
    position: absolute;
    right: -2px; bottom: -2px;
    width: 12px; height: 12px;
    border-radius: 50%;
    background: var(--online);
    border: 3px solid var(--bg-rail);
    box-shadow: 0 0 8px var(--online);
  }
  .nav-btn[data-label]:hover::after {
    content: attr(data-label);
    position: absolute;
    left: calc(100% + 14px);
    top: 50%;
    transform: translateY(-50%);
    background: var(--bg-surface-2);
    border: 1px solid var(--border-strong);
    color: var(--fg);
    padding: 6px 10px;
    border-radius: 8px;
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
    box-shadow: var(--shadow-lift);
    z-index: 100;
    pointer-events: none;
  }

  /* Labels under icons — only shown in the mobile bottom bar. */
  .nav-lbl { display: none; }

  /* ---- Mobile: the rail becomes a bottom tab bar ---- */
  @media (max-width: 720px) {
    .rail {
      flex-direction: row;
      align-items: center;
      justify-content: space-around;
      position: fixed;
      left: 0; right: 0; bottom: 0;
      height: calc(60px + env(safe-area-inset-bottom, 0px));
      padding: 0 4px env(safe-area-inset-bottom, 0px);
      border-right: none;
      border-top: 1px solid var(--border-strong);
      gap: 0;
      z-index: 60;
    }
    .brand, .rail-divider, .rail-spacer, .status-pill { display: none; }
    .nav-btn {
      flex-direction: column;
      gap: 2px;
      width: auto; height: auto;
      padding: 4px 8px;
      border-radius: 10px;
    }
    .nav-btn::before { display: none; }
    .nav-btn :global(svg) { width: 22px; height: 22px; }
    .nav-btn[data-label]:hover::after { display: none; } /* no tooltips on touch */
    .nav-lbl { display: block; font-size: 10px; font-weight: 600; }
    .me { width: 34px; height: 34px; font-size: 13px; }
  }
</style>
