<script lang="ts">
  /**
   * Music (the Navidrome add-on, lib/music.ts): the phone tab / the desktop
   * side column. Home (new albums, playlists, favourites), search, album,
   * artist and playlist pages; songs play in the app's one player
   * (TopPlayer). Opened from a chat's paperclip, a track goes to that chat.
   */
  import { t, tr } from "../i18n";
  import { showToast } from "./Toast.svelte";
  import { activeTab, chats, selectChat, activeId } from "../stores";
  import { openSettings } from "../panels/settings";
  import { nowPlaying, repeatAll, expandPlayer } from "../player";
  import { fmtDuration } from "../media";
  import {
    musicOn, musicStatus, musicSources, musicSource, musicSendTo, refreshMusic, coverUrl,
    search, albumList, album, artist, playlists, playlist, starred, randomSongs,
    playSongs, shuffleSongs, downloadSong, sendCard,
    hosting, following, startListening, stopHosting, leaveListening,
    type Src, type Song, type Album, type Artist, type Playlist,
  } from "../music";
  import { settings } from "../theme";
  import { icons } from "../icons";

  type View =
    | { kind: "home" }
    | { kind: "search"; q: string }
    | { kind: "album"; id: string; title: string }
    | { kind: "artist"; id: string; title: string }
    | { kind: "playlist"; id: string; title: string }
    | { kind: "starred" };

  let stack = $state<View[]>([{ kind: "home" }]);
  const view = $derived(stack[stack.length - 1]);
  let q = $state("");
  let busy = $state(false);
  let err = $state("");

  // What the current view shows.
  let songs = $state<Song[]>([]);
  let albums = $state<Album[]>([]);
  let artists = $state<Artist[]>([]);
  let lists = $state<Playlist[]>([]);
  let fav = $state<Song[]>([]);
  let frequent = $state<Album[]>([]);
  let head = $state<{ title: string; sub: string; cover?: string }>({ title: "", sub: "" });

  const src = $derived<Src>($musicSources.includes($musicSource) ? $musicSource : ($musicSources[0] ?? "server"));
  const ready = $derived($musicSources.length > 0);

  function push(v: View) { stack = [...stack, v]; }
  function back() { if (stack.length > 1) stack = stack.slice(0, -1); }

  let seq = 0;
  async function load(v: View, s: Src) {
    const my = ++seq;
    busy = true; err = "";
    songs = []; albums = []; artists = []; lists = []; fav = []; frequent = [];
    try {
      if (v.kind === "home") {
        const [n, f, p, st] = await Promise.all([
          albumList(s, "newest", 18), albumList(s, "frequent", 12).catch(() => []),
          playlists(s).catch(() => []), starred(s).catch(() => []),
        ]);
        if (my !== seq) return;
        albums = n; frequent = f; lists = p; fav = st.slice(0, 8);
      } else if (v.kind === "search") {
        const r = await search(s, v.q);
        if (my !== seq) return;
        songs = r.songs; albums = r.albums; artists = r.artists;
      } else if (v.kind === "album") {
        const r = await album(s, v.id);
        if (my !== seq) return;
        songs = r.songs;
        head = { title: r.album.name, sub: [r.album.artist, r.album.year].filter(Boolean).join(" · "), cover: r.album.coverArt };
      } else if (v.kind === "artist") {
        const r = await artist(s, v.id);
        if (my !== seq) return;
        albums = r.albums;
        head = { title: r.artist.name, sub: tr("{n} albums", { n: r.albums.length }), cover: r.artist.coverArt };
      } else if (v.kind === "playlist") {
        const r = await playlist(s, v.id);
        if (my !== seq) return;
        songs = r.songs;
        head = { title: r.playlist.name, sub: tr("{n} tracks", { n: r.songs.length }), cover: r.playlist.coverArt };
      } else if (v.kind === "starred") {
        const r = await starred(s);
        if (my !== seq) return;
        songs = r;
        head = { title: tr("Favourites"), sub: tr("{n} tracks", { n: r.length }) };
      }
    } catch (e) {
      if (my === seq) err = String(e);
    } finally {
      if (my === seq) busy = false;
    }
  }
  $effect(() => { if (ready && $musicOn) void load(view, src); });
  $effect(() => { if ($musicOn) void refreshMusic(); });

  let qTimer: ReturnType<typeof setTimeout> | null = null;
  function onQuery() {
    if (qTimer) clearTimeout(qTimer);
    qTimer = setTimeout(() => {
      const v = q.trim();
      const base = stack.filter((x) => x.kind !== "search");
      stack = v ? [...base, { kind: "search", q: v }] : (base.length ? base : [{ kind: "home" }]);
    }, 320);
  }

  // Covers: loopback URLs from Rust, set once known.
  function cover(node: HTMLImageElement, a: [Src, string | undefined, number]) {
    let cur = a;
    const set = (x: [Src, string | undefined, number]) => {
      node.classList.remove("ok");
      if (!x[1]) return;
      void coverUrl(x[0], x[1], x[2]).then((u) => { if (cur === x && u) node.src = u; });
    };
    node.onload = () => node.classList.add("ok");
    set(a);
    return { update(n: [Src, string | undefined, number]) { cur = n; set(n); } };
  }

  const sendTo = $derived($musicSendTo);
  const sendName = $derived(sendTo ? ($chats.find((c) => c.id === sendTo)?.name ?? sendTo) : "");

  async function send(s: Song) {
    const chat = sendTo ?? $activeId;
    if (!chat) { showToast(tr("Open a chat first")); return; }
    try {
      await sendCard(chat, s);
      showToast(tr("Sent"));
      if (sendTo) { musicSendTo.set(null); activeTab.set("chats"); void selectChat(chat); }
    } catch (e) { showToast(String(e)); }
  }
  async function save(s: Song) {
    showToast(tr("Downloading…"));
    try { const p = await downloadSong(s); showToast(tr("Saved: {p}", { p }), 6000); } catch (e) { showToast(String(e)); }
  }
  async function together() {
    if ($hosting) { stopHosting(); showToast(tr("Listening together stopped")); return; }
    const chat = $activeId;
    if (!chat || chat.startsWith("grp_") || chat === "__saved__") { showToast(tr("Open a one-to-one chat, then press it again")); return; }
    try { await startListening(chat); showToast(tr("Invite sent — they press “Join”")); } catch (e) { showToast(String(e instanceof Error ? e.message : e)); }
  }

  const np = $derived($nowPlaying?.nd ? $nowPlaying : null);
</script>

<aside class="panel music">
  <div class="head">
    {#if stack.length > 1}
      <button class="ic-btn" onclick={back} aria-label={$t("Back")}><svg viewBox="0 0 24 24"><path d="M15 18l-6-6 6-6"/></svg></button>
    {/if}
    <h2>{$t("Music")}</h2>
    {#if $musicSources.length > 1}
      <div class="srcs" role="tablist">
        {#each $musicSources as s (s)}
          <button class:on={src === s} onclick={() => { musicSource.set(s); stack = [{ kind: "home" }]; q = ""; }}>{s === "server" ? $t("Server") : $t("Mine")}</button>
        {/each}
      </div>
    {/if}
    <button class="ic-btn" onclick={() => openSettings("music")} title={$t("Settings")} aria-label={$t("Settings")}>
      {@html icons.settings}
    </button>
  </div>

  {#if sendTo}
    <div class="send-to">
      <span>{$t("Pick a track to send to “{n}”", { n: sendName })}</span>
      <button onclick={() => { musicSendTo.set(null); activeTab.set("chats"); }}>{$t("Cancel")}</button>
    </div>
  {/if}

  {#if !$musicOn}
    <div class="empty">
      <div class="big">🎵</div>
      <p>{$t("Music from Navidrome: listen, download, send tracks to chats and listen together.")}</p>
      <button class="primary" onclick={() => settings.update((s) => ({ ...s, navidrome: true }))}>{$t("Turn on")}</button>
    </div>
  {:else if !ready}
    <div class="empty">
      <div class="big">🎵</div>
      <p>{$t("No music here yet: this server has no Navidrome, and yours isn't connected.")}</p>
      <button class="primary" onclick={() => openSettings("music")}>{$t("Connect your Navidrome")}</button>
    </div>
  {:else}
    <div class="search">
      <svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="7"/><path d="M21 21l-4.3-4.3"/></svg>
      <input bind:value={q} oninput={onQuery} placeholder={$t("Songs, albums, artists")} spellcheck="false" />
      {#if q}<button class="x" onclick={() => { q = ""; onQuery(); }} aria-label={$t("Clear")}>×</button>{/if}
    </div>

    {#if np}
      <div class="np">
        <button class="np-main" onclick={expandPlayer}>
          {#if np.cover}<img src={np.cover} alt="" class="ok" />{:else}<span class="ph">♪</span>{/if}
          <span class="np-t"><b>{np.title}</b><small>{np.nd?.artist ?? ""}</small></span>
        </button>
        <button class="ic-btn" class:on={$repeatAll} onclick={() => repeatAll.update((v) => !v)} title={$t("Repeat")} aria-label={$t("Repeat")}>
          <svg viewBox="0 0 24 24"><path d="M17 2l4 4-4 4"/><path d="M3 11V9a3 3 0 0 1 3-3h15"/><path d="M7 22l-4-4 4-4"/><path d="M21 13v2a3 3 0 0 1-3 3H3"/></svg>
        </button>
        {#if $following}
          <button class="lt on" onclick={leaveListening}>👥 {$t("Leave")}</button>
        {:else}
          <button class="lt" class:on={!!$hosting} onclick={together} title={$t("Listen together")}>👥 {$hosting ? $t("Stop") : $t("Together")}</button>
        {/if}
      </div>
      {#if $following}<div class="lt-note">{$t("Listening together with @{h}", { h: $following.host })}</div>{/if}
    {/if}

    <div class="body">
      {#if view.kind === "album" || view.kind === "artist" || view.kind === "playlist" || view.kind === "starred"}
        <div class="hero">
          <img use:cover={[src, head.cover, 300]} alt="" />
          <div class="hero-t"><h3>{head.title}</h3><small>{head.sub}</small>
            {#if songs.length}
              <div class="hero-acts">
                <button class="primary" onclick={() => playSongs(songs, 0)}>▶ {$t("Play")}</button>
                <button onclick={() => shuffleSongs(songs)}>⤮ {$t("Shuffle")}</button>
              </div>
            {/if}
          </div>
        </div>
      {/if}

      {#if busy}<div class="loading"><span class="spin"></span></div>{/if}
      {#if err}<div class="err">{err}</div>{/if}

      {#if view.kind === "home" && !busy}
        <div class="quick">
          <button onclick={async () => { try { await shuffleSongs(await randomSongs(src, 60)); } catch (e) { showToast(String(e)); } }}>⤮ {$t("Random")}</button>
          <button onclick={() => push({ kind: "starred" })}>★ {$t("Favourites")}</button>
        </div>
      {/if}

      {#if artists.length}
        <h4>{$t("Artists")}</h4>
        <div class="chips">
          {#each artists as a (a.id)}
            <button class="chip" onclick={() => push({ kind: "artist", id: a.id, title: a.name })}>{a.name}</button>
          {/each}
        </div>
      {/if}

      {#if albums.length}
        <h4>{view.kind === "home" ? $t("New albums") : $t("Albums")}</h4>
        <div class="grid">
          {#each albums as a (a.id)}
            <button class="tile" onclick={() => push({ kind: "album", id: a.id, title: a.name })}>
              <span class="art"><img use:cover={[a.src, a.coverArt, 300]} alt="" /></span>
              <b>{a.name}</b><small>{a.artist}{a.year ? ` · ${a.year}` : ""}</small>
            </button>
          {/each}
        </div>
      {/if}

      {#if frequent.length}
        <h4>{$t("Most played")}</h4>
        <div class="grid">
          {#each frequent as a (a.id)}
            <button class="tile" onclick={() => push({ kind: "album", id: a.id, title: a.name })}>
              <span class="art"><img use:cover={[a.src, a.coverArt, 300]} alt="" /></span>
              <b>{a.name}</b><small>{a.artist}</small>
            </button>
          {/each}
        </div>
      {/if}

      {#if lists.length}
        <h4>{$t("Playlists")}</h4>
        {#each lists as p (p.id)}
          <button class="row pl" onclick={() => push({ kind: "playlist", id: p.id, title: p.name })}>
            <span class="thumb"><img use:cover={[p.src, p.coverArt, 120]} alt="" /></span>
            <span class="rt"><b>{p.name}</b><small>{tr("{n} tracks", { n: p.songCount ?? 0 })}</small></span>
          </button>
        {/each}
      {/if}

      {#if fav.length}
        <h4>{$t("Favourites")}</h4>
        {#each fav as s, i (s.id)}
          {@render songRow(s, i, fav)}
        {/each}
      {/if}

      {#if songs.length}
        {#if view.kind === "search"}<h4>{$t("Songs")}</h4>{/if}
        {#each songs as s, i (s.id + ":" + i)}
          {@render songRow(s, i, songs)}
        {/each}
      {/if}

      {#if !busy && !err && view.kind === "search" && !songs.length && !albums.length && !artists.length}
        <div class="none">{$t("Nothing found")}</div>
      {/if}
    </div>
  {/if}
</aside>

{#snippet songRow(s: Song, i: number, list: Song[])}
  <div class="row song" class:playing={$nowPlaying?.nd?.id === s.id}>
    <button class="row-main" onclick={() => playSongs(list, i)}>
      <span class="thumb"><img use:cover={[s.src, s.coverArt, 120]} alt="" /></span>
      <span class="rt"><b>{s.title}</b><small>{s.artist}{s.album && view.kind !== "album" ? ` — ${s.album}` : ""}</small></span>
      <span class="dur">{fmtDuration(s.duration)}</span>
    </button>
    <button class="ic-btn" class:accent={!!sendTo} onclick={() => send(s)} title={sendTo ? $t("Send") : $t("Send to the open chat")} aria-label={$t("Send")}>
      <svg viewBox="0 0 24 24"><path d="M22 2L11 13"/><path d="M22 2l-7 20-4-9-9-4z"/></svg>
    </button>
    <button class="ic-btn" onclick={() => save(s)} title={$t("Download")} aria-label={$t("Download")}>
      <svg viewBox="0 0 24 24"><path d="M12 3v12"/><path d="M7 10l5 5 5-5"/><path d="M5 21h14"/></svg>
    </button>
  </div>
{/snippet}

<style>
  .panel {
    background: var(--bg-list);
    border-right: 1px solid var(--border);
    display: flex; flex-direction: column;
    min-width: 0; min-height: 0;
  }
  .head { display: flex; align-items: center; gap: 6px; padding: 12px 12px 8px 16px; }
  .head h2 { margin: 0; font-size: 15.5px; font-weight: 700; flex: 1; }
  .ic-btn { width: 34px; height: 34px; flex: none; display: grid; place-items: center; border: none; background: transparent; color: var(--fg-muted);
    border-radius: calc(10px * var(--rk, 1)); cursor: pointer; }
  .ic-btn:hover { background: var(--bg-hover); color: var(--fg); }
  .ic-btn.on, .ic-btn.accent { color: var(--accent); }
  .ic-btn :global(svg), .search svg { width: 18px; height: 18px; fill: none; stroke: currentColor; stroke-width: 1.9; stroke-linecap: round; stroke-linejoin: round; }
  .srcs { display: flex; gap: 2px; padding: 2px; border: 1px solid var(--border); border-radius: calc(10px * var(--rk, 1)); }
  .srcs button { border: none; background: transparent; color: var(--fg-muted); font: inherit; font-size: 12px; font-weight: 600; padding: 4px 9px; border-radius: calc(8px * var(--rk, 1)); cursor: pointer; }
  .srcs button.on { background: var(--accent-soft); color: var(--accent); }

  .send-to { margin: 0 12px 8px; padding: 8px 10px; display: flex; align-items: center; gap: 8px; border-radius: calc(12px * var(--rk, 1));
    background: var(--accent-soft); color: var(--fg); font-size: 13px; }
  .send-to span { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .send-to button { border: none; background: transparent; color: var(--accent); font: inherit; font-weight: 700; cursor: pointer; }

  .search { margin: 0 12px 8px; display: flex; align-items: center; gap: 8px; padding: 0 10px; height: 38px;
    background: var(--bg-input, var(--bg-hover)); border: 1px solid var(--border); border-radius: calc(12px * var(--rk, 1)); color: var(--fg-muted); }
  .search input { flex: 1; min-width: 0; border: none; background: transparent; color: var(--fg); font: inherit; font-size: 14px; outline: none; }
  .search .x { border: none; background: none; color: var(--fg-muted); font-size: 18px; cursor: pointer; }

  .np { margin: 0 12px 8px; display: flex; align-items: center; gap: 6px; padding: 6px; border: 1px solid var(--border);
    border-radius: calc(14px * var(--rk, 1)); background: var(--bg-elev, var(--bg-hover)); }
  .np-main { flex: 1; min-width: 0; display: flex; align-items: center; gap: 10px; border: none; background: transparent; color: var(--fg); text-align: left; cursor: pointer; padding: 0; }
  .np-main img, .np-main .ph { width: 40px; height: 40px; border-radius: calc(9px * var(--rk, 1)); object-fit: cover; flex: none; }
  .np-main .ph { display: grid; place-items: center; background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #fff; }
  .np-t { min-width: 0; display: flex; flex-direction: column; }
  .np-t b, .np-t small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .np-t b { font-size: 13.5px; } .np-t small { font-size: 12px; color: var(--fg-muted); }
  .lt { flex: none; height: 32px; padding: 0 10px; border: 1px solid var(--border); background: transparent; color: var(--fg); font: inherit; font-size: 12.5px;
    font-weight: 600; border-radius: calc(10px * var(--rk, 1)); cursor: pointer; }
  .lt.on { border-color: var(--accent); color: var(--accent); background: var(--accent-soft); }
  .lt-note { margin: -4px 14px 8px; font-size: 12px; color: var(--accent); }

  .body { flex: 1; min-height: 0; overflow-y: auto; padding: 0 8px calc(16px + env(safe-area-inset-bottom, 0px)); }
  h4 { margin: 14px 8px 6px; font-size: 12px; font-weight: 700; letter-spacing: .04em; text-transform: uppercase; color: var(--fg-muted); }
  .quick { display: flex; gap: 6px; padding: 4px 4px 0; }
  .quick button, .hero-acts button { flex: 1; height: 36px; border: 1px solid var(--border); background: transparent; color: var(--fg); font: inherit; font-size: 13px;
    font-weight: 600; border-radius: calc(11px * var(--rk, 1)); cursor: pointer; }
  .quick button:hover, .hero-acts button:hover { background: var(--bg-hover); }
  .primary { background: linear-gradient(135deg, var(--accent), var(--accent-2)) !important; color: #061015 !important; border: none !important; }

  .chips { display: flex; flex-wrap: wrap; gap: 6px; padding: 0 4px; }
  .chip { border: 1px solid var(--border); background: transparent; color: var(--fg); font: inherit; font-size: 13px; padding: 6px 11px; border-radius: 999px; cursor: pointer; }
  .chip:hover { border-color: var(--accent); color: var(--accent); }

  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(118px, 1fr)); gap: 10px; padding: 0 4px; }
  .tile { display: flex; flex-direction: column; gap: 3px; border: none; background: transparent; color: var(--fg); text-align: left; padding: 4px; cursor: pointer;
    border-radius: calc(12px * var(--rk, 1)); min-width: 0; }
  .tile:hover { background: var(--bg-hover); }
  .tile b, .tile small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tile b { font-size: 13px; } .tile small { font-size: 11.5px; color: var(--fg-muted); }
  .art { display: block; width: 100%; aspect-ratio: 1; border-radius: calc(10px * var(--rk, 1)); overflow: hidden;
    background: linear-gradient(135deg, color-mix(in srgb, var(--accent) 35%, transparent), color-mix(in srgb, var(--accent-2) 35%, transparent)); }
  .art img, .thumb img, .hero img { width: 100%; height: 100%; object-fit: cover; display: block; opacity: 0; transition: opacity .2s; }
  .panel :global(img.ok) { opacity: 1 !important; }

  .row { display: flex; align-items: center; gap: 4px; width: 100%; border-radius: calc(12px * var(--rk, 1)); }
  .row.pl { border: none; background: transparent; color: var(--fg); text-align: left; padding: 6px; cursor: pointer; gap: 10px; }
  .row.pl:hover, .row.song:hover { background: var(--bg-hover); }
  .row-main { flex: 1; min-width: 0; display: flex; align-items: center; gap: 10px; border: none; background: transparent; color: var(--fg); text-align: left; padding: 6px; cursor: pointer; }
  .thumb { width: 42px; height: 42px; flex: none; border-radius: calc(8px * var(--rk, 1)); overflow: hidden;
    background: linear-gradient(135deg, color-mix(in srgb, var(--accent) 35%, transparent), color-mix(in srgb, var(--accent-2) 35%, transparent)); }
  .rt { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .rt b, .rt small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .rt b { font-size: 14px; font-weight: 600; } .rt small { font-size: 12px; color: var(--fg-muted); }
  .row.song.playing .rt b { color: var(--accent); }
  .dur { font-size: 12px; color: var(--fg-muted); font-variant-numeric: tabular-nums; }

  .hero { display: flex; gap: 14px; align-items: flex-end; padding: 6px 6px 4px; }
  .hero img { width: 110px; height: 110px; flex: none; border-radius: calc(14px * var(--rk, 1));
    background: linear-gradient(135deg, color-mix(in srgb, var(--accent) 35%, transparent), color-mix(in srgb, var(--accent-2) 35%, transparent)); opacity: 1; }
  .hero-t { min-width: 0; flex: 1; display: flex; flex-direction: column; gap: 4px; }
  .hero-t h3 { margin: 0; font-size: 18px; line-height: 1.2; }
  .hero-t small { color: var(--fg-muted); font-size: 12.5px; }
  .hero-acts { display: flex; gap: 6px; margin-top: 6px; }

  .empty { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px; padding: 24px; text-align: center; color: var(--fg-muted); }
  .empty .big { font-size: 44px; }
  .empty p { margin: 0; max-width: 300px; line-height: 1.45; font-size: 14px; }
  .empty button { height: 40px; padding: 0 18px; border-radius: calc(12px * var(--rk, 1)); font: inherit; font-weight: 700; cursor: pointer; }
  .loading { display: grid; place-items: center; padding: 20px; }
  .spin { width: 20px; height: 20px; border-radius: 50%; border: 2px solid var(--accent-soft); border-top-color: var(--accent); animation: mp-spin .7s linear infinite; }
  @keyframes mp-spin { to { transform: rotate(360deg); } }
  .err { margin: 10px 8px; padding: 10px 12px; border-radius: calc(12px * var(--rk, 1)); background: color-mix(in srgb, var(--danger, #FF6B81) 14%, transparent);
    color: var(--danger, #FF6B81); font-size: 13px; word-break: break-word; }
  .none { padding: 24px; text-align: center; color: var(--fg-muted); font-size: 13.5px; }
</style>
