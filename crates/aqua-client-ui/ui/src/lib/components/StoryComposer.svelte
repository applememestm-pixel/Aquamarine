<script lang="ts">
  /**
   * Before a story goes up (Telegram): the picture / video, a caption, who
   * may see it (everyone, my contacts, close friends, chosen people) and
   * how long it lives (6 / 12 / 24 / 48 h). The choice is remembered.
   */
  import { t, tr } from "../i18n";
  import { assetUrl } from "../tauri";
  import { onBack } from "../back";
  import { chats } from "../stores";
  import { avatars } from "../people";
  import { storyDraft } from "../storyFeed";
  import { postStory, closeFriends, storyDefaults, type StoryAudience } from "../stories";
  import { showToast } from "./Toast.svelte";
  import { IS_MOBILE } from "../platform";

  const draft = $derived($storyDraft);
  const isVideo = $derived(!!draft && /\.(mp4|webm|mov|m4v|mkv|3gp)$/i.test(draft.path));
  let caption = $state("");
  let aud = $state<StoryAudience>($storyDefaults.aud);
  let sel = $state<string[]>([...$storyDefaults.sel]);
  let hours = $state($storyDefaults.hours);
  /** Choosing people: for close friends or for "selected". */
  let picking = $state<null | "close" | "selected">(null);
  let pickSet = $state<string[]>([]);
  let q = $state("");
  let posting = $state(false);

  $effect(() => {
    if (!draft) return;
    caption = ""; picking = null; posting = false;
    aud = $storyDefaults.aud; sel = [...$storyDefaults.sel]; hours = $storyDefaults.hours;
  });
  const isOpen = $derived(!!draft);
  $effect(() => { if (isOpen) return onBack(close); });
  const isPicking = $derived(!!picking);
  $effect(() => { if (isPicking) return onBack(() => (picking = null)); });

  const people = $derived($chats.filter((c) => !c.id.startsWith("grp_") && !c.id.startsWith("__") && !c.channel));
  const shown = $derived(q.trim() ? people.filter((c) => c.name.toLowerCase().includes(q.trim().toLowerCase()) || c.id.includes(q.trim().toLowerCase())) : people);

  function close() { storyDraft.set(null); }
  function openPick(k: "close" | "selected") {
    picking = k;
    pickSet = k === "close" ? [...$closeFriends] : [...sel];
    q = "";
  }
  function donePick() {
    if (picking === "close") { closeFriends.set([...pickSet]); aud = "close"; }
    else { sel = [...pickSet]; aud = "selected"; }
    picking = null;
  }
  const toggle = (id: string) => (pickSet = pickSet.includes(id) ? pickSet.filter((x) => x !== id) : [...pickSet, id]);

  async function post() {
    const d = draft;
    if (!d || posting) return;
    if (aud === "selected" && !sel.length) { openPick("selected"); return; }
    posting = true;
    storyDefaults.set({ aud, sel: [...sel], hours });
    const opts = { caption, aud, sel: [...sel], hours };
    close();
    showToast(tr("Posting the story…"), 30000);
    try {
      await postStory(d.path, opts);
      showToast(tr("Story posted — it's visible for {n} hours", { n: hours }), 3500);
    } catch (e) {
      showToast(`${tr("Could not post the story")}: ${e}`, 6000);
    }
  }

  const AUD: [StoryAudience, string, string][] = [
    ["all", "Everyone", "Everyone you chat with"],
    ["contacts", "My contacts", "Only people in your contacts"],
    ["close", "Close friends", "A green ring — only for them"],
    ["selected", "Selected people", "Only the people you choose"],
  ];
</script>

{#if draft}
  <div class="sc" data-phone-full class:phone={IS_MOBILE} role="dialog" aria-modal="true" aria-label={$t("New story")} data-own-back>
    <div class="card">
      {#if !picking}
        <div class="head">
          <button class="ic" data-x onclick={close} aria-label={$t("Close")}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>
          </button>
          <b>{$t("New story")}</b>
        </div>
        <div class="body">
          <div class="pv">
            {#if isVideo}
              <!-- svelte-ignore a11y_media_has_caption -->
              <video src={assetUrl(draft.path)} autoplay loop muted playsinline></video>
            {:else}
              <img class="bg" src={assetUrl(draft.path)} alt="" aria-hidden="true" />
              <img class="fg" src={assetUrl(draft.path)} alt="" />
            {/if}
          </div>
          <div class="side">
            <input class="cap" maxlength="200" placeholder={$t("Add a caption…")} bind:value={caption}
              onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); void post(); } }} />
            <div class="lbl">{$t("Who can see this story")}</div>
            <div class="rows">
              {#each AUD as [k, label, hint] (k)}
                <div class="row" class:on={aud === k}>
                  <button class="pick" onclick={() => { if (k === "selected" && !sel.length) openPick("selected"); else aud = k; }}>
                    <span class="radio" class:green={k === "close"}></span>
                    <span class="txt"><b>{$t(label)}</b><small>{k === "close" ? $t("{n} people", { n: $closeFriends.length }) : k === "selected" ? $t("{n} people", { n: sel.length }) : $t(hint)}</small></span>
                  </button>
                  {#if k === "close" || k === "selected"}
                    <button class="edit" onclick={() => openPick(k)}>{$t(k === "close" ? "Edit" : "Choose")}</button>
                  {/if}
                </div>
              {/each}
            </div>
            <div class="lbl">{$t("Keep for")}</div>
            <div class="chips">
              {#each [6, 12, 24, 48] as h (h)}
                <button class="chip" class:on={hours === h} onclick={() => (hours = h)}>{$t("{n} h", { n: h })}</button>
              {/each}
            </div>
            <button class="go" disabled={posting} onclick={post}>{$t("Post story")}</button>
          </div>
        </div>
      {:else}
        <div class="head">
          <button class="ic" onclick={() => (picking = null)} aria-label={$t("Back")}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
          </button>
          <b>{$t(picking === "close" ? "Close friends" : "Selected people")}</b>
          <button class="done" onclick={donePick}>{$t("Done")}</button>
        </div>
        <div class="plist">
          <input class="cap" placeholder={$t("Search")} bind:value={q} />
          {#if !shown.length}<div class="empty">{$t("No one here yet")}</div>{/if}
          {#each shown as c (c.id)}
            <button class="person" onclick={() => toggle(c.id)}>
              <span class="pa" style:--gradient={c.gradient} style:background-image={$avatars[c.id] ? `url("${$avatars[c.id]}")` : undefined}>{$avatars[c.id] ? "" : c.letter}</span>
              <span class="pn"><b>{c.name}</b><small>@{c.id}</small></span>
              <span class="check" class:on={pickSet.includes(c.id)} class:green={picking === "close"}></span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .sc { position: fixed; inset: 0; z-index: 10096; display: grid; place-items: center; background: rgba(0,0,0,.72); }
  .card { width: min(820px, calc(100vw - 32px)); height: min(640px, calc(100% - 48px)); display: flex; flex-direction: column; background: var(--bg-shell); color: var(--fg);
    border: 1px solid var(--border-strong); border-radius: calc(18px * var(--rk, 1)); overflow: hidden; }
  .sc.phone { background: var(--bg-shell); padding: var(--sat, env(safe-area-inset-top, 0px)) 0 var(--sab, env(safe-area-inset-bottom, 0px)); box-sizing: border-box; }
  .sc.phone .card { width: 100%; height: 100%; border: none; border-radius: 0; }
  .head { display: flex; align-items: center; gap: 8px; padding: 8px 10px; border-bottom: 1px solid var(--border); flex-shrink: 0; }
  .head b { flex: 1; font-size: 16px; }
  .ic { width: 38px; height: 38px; border: none; border-radius: 50%; background: transparent; color: var(--fg); display: grid; place-items: center; cursor: pointer; }
  .ic:hover { background: var(--bg-hover); }
  .ic svg { width: 21px; height: 21px; }
  .done { border: none; background: transparent; color: var(--accent); font: 700 14.5px var(--font-ui, inherit); padding: 8px 10px; cursor: pointer; }
  .body { flex: 1; min-height: 0; display: grid; grid-template-columns: minmax(0, 300px) 1fr; gap: 18px; padding: 16px; }
  .phone .body { grid-template-columns: 1fr; grid-template-rows: minmax(160px, 1fr) auto; overflow-y: auto; gap: 12px; padding: 12px; }
  .pv { position: relative; display: grid; place-items: center; background: #000; border-radius: calc(14px * var(--rk, 1)); overflow: hidden; min-height: 0; aspect-ratio: 9 / 16; max-height: 100%; justify-self: center; height: 100%; }
  .pv img, .pv video { grid-area: 1 / 1; width: 100%; height: 100%; object-fit: contain; }
  .pv img.bg { object-fit: cover; filter: blur(24px) brightness(.55); transform: scale(1.15); }
  .pv img.fg, .pv video { position: relative; z-index: 1; }
  .side { display: flex; flex-direction: column; gap: 10px; min-height: 0; overflow-y: auto; }
  .cap { width: 100%; padding: 11px 14px; border-radius: calc(12px * var(--rk, 1)); border: 1px solid var(--border-strong); background: var(--bg-surface); color: var(--fg); font: 15px var(--font-ui, inherit); outline: none; }
  .cap:focus { border-color: var(--accent); }
  .lbl { margin-top: 4px; font-size: 12.5px; font-weight: 700; color: var(--accent); text-transform: uppercase; letter-spacing: .03em; }
  .rows { display: flex; flex-direction: column; border-radius: calc(12px * var(--rk, 1)); background: var(--bg-surface); overflow: hidden; }
  .row { display: flex; align-items: center; }
  .row + .row { border-top: 1px solid var(--border); }
  .pick { flex: 1; display: flex; align-items: center; gap: 12px; padding: 10px 12px; border: none; background: transparent; color: var(--fg); text-align: left; cursor: pointer; }
  .pick:hover { background: var(--bg-hover); }
  .txt { display: flex; flex-direction: column; min-width: 0; }
  .txt b { font-size: 14.5px; font-weight: 600; }
  .txt small { font-size: 12.5px; color: var(--fg-muted); }
  .radio { width: 20px; height: 20px; border-radius: 50%; border: 2px solid var(--border-strong); flex-shrink: 0; box-sizing: border-box; }
  .row.on .radio { border: 6px solid var(--accent); }
  .row.on .radio.green { border-color: #34C759; }
  .edit { border: none; background: transparent; color: var(--accent); font: 600 13.5px var(--font-ui, inherit); padding: 10px 12px; cursor: pointer; }
  .chips { display: flex; gap: 8px; flex-wrap: wrap; }
  .chip { padding: 8px 14px; border-radius: 999px; border: 1px solid var(--border-strong); background: var(--bg-surface); color: var(--fg); font: 600 13.5px var(--font-ui, inherit); cursor: pointer; }
  .chip.on { background: var(--accent-soft); border-color: var(--accent); color: var(--accent); }
  .go { margin-top: auto; padding: 13px; border: none; border-radius: calc(12px * var(--rk, 1)); background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070c; font: 800 15px var(--font-ui, inherit); cursor: pointer; }
  .go:disabled { opacity: .6; }
  .plist { flex: 1; min-height: 0; overflow-y: auto; padding: 10px; display: flex; flex-direction: column; gap: 2px; }
  .plist .cap { margin-bottom: 6px; }
  .empty { padding: 30px; text-align: center; color: var(--fg-muted); }
  .person { display: flex; align-items: center; gap: 12px; padding: 8px; border: none; background: transparent; color: var(--fg); border-radius: calc(10px * var(--rk, 1)); cursor: pointer; text-align: left; }
  .person:hover { background: var(--bg-hover); }
  .pa { width: 40px; height: 40px; border-radius: var(--ava-r, 50%); flex-shrink: 0; display: grid; place-items: center; background: var(--gradient, var(--bg-surface-2)) center / cover; color: rgba(6,7,12,.85); font-weight: 800; }
  .pn { flex: 1; display: flex; flex-direction: column; min-width: 0; }
  .pn b { font-size: 14.5px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .pn small { font-size: 12.5px; color: var(--fg-muted); }
  .check { width: 22px; height: 22px; border-radius: 50%; border: 2px solid var(--border-strong); box-sizing: border-box; flex-shrink: 0; }
  .check.on { background: var(--accent); border-color: var(--accent); box-shadow: inset 0 0 0 3px var(--bg-shell); }
  .check.on.green { background: #34C759; border-color: #34C759; }
</style>
