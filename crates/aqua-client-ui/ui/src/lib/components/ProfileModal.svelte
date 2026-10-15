<script module lang="ts">
  import { writable, get as getStore } from "svelte/store";

  // Null closed, string = username whose profile we're showing,
  // or "__me__" for the current user.
  export const profileState = writable<string | null>(null);

  export function openProfile(username = "__me__") { profileState.set(username); }
  export function closeProfile() { profileState.set(null); }
</script>

<script lang="ts">
  import ColorInput from "./ColorInput.svelte";
  import { onMount } from "svelte";
  import { profile, updateProfile, type Profile } from "../profile";
  import { contacts, saveContact, getContact } from "../contacts";
  import { info } from "../stores";
  import { api, bytesToDataUrl } from "../tauri";
  import { icons } from "../icons";
  import { openFileDialog } from "../dialog";
  import { showToast } from "./Toast.svelte";
  import { broadcastPresence, peerPresence, myActivity, shareActivity, myOnline, activityVerb, activityLine } from "../presence";
  import { peerCards, refreshPeer, publishMyProfile, setMyImage, setMyImageBytes, clearMyImage, setMyMediaFile, clearMyMusic, str, clans } from "../people";
  import MediaPlayer from "./MediaPlayer.svelte";
  import { streamSrc } from "../media";
  import ClanTag from "./ClanTag.svelte";
  import { privacy, PRIV_ITEMS, allowed, exceptionOf, setException, exceptionCount, contactSet, type PrivKey, type Exception } from "../privacy";
  import { t, tr } from "../i18n";
  import { blocked, setBlocked } from "../ghost";
  import { cropImage } from "./ImageCropper.svelte";
  import type { Activity } from "../types";
  import { openRegistration, registrationDismissed } from "./RegistrationWizard.svelte";
  import { playedTop, played, gameGradient } from "../played";

  let pfTab = $state<"board" | "activity">("board");
  function hostOf(url: string): string {
    try { return new URL(url).hostname.replace(/^www\./, ""); } catch { return url; }
  }
  const LINK_ICONS: [RegExp, string][] = [
    [/github\.com/, `<svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 2a10 10 0 0 0-3.2 19.5c.5.1.7-.2.7-.5v-1.7c-2.8.6-3.4-1.3-3.4-1.3-.5-1.2-1.1-1.5-1.1-1.5-.9-.6.1-.6.1-.6 1 .1 1.5 1 1.5 1 .9 1.5 2.4 1.1 2.9.8.1-.7.4-1.1.6-1.3-2.2-.3-4.6-1.1-4.6-5 0-1.1.4-2 1-2.7-.1-.3-.4-1.3.1-2.7 0 0 .8-.3 2.8 1a9.6 9.6 0 0 1 5 0c1.9-1.3 2.7-1 2.7-1 .6 1.4.2 2.4.1 2.7.7.7 1 1.6 1 2.7 0 3.9-2.3 4.7-4.6 5 .4.3.7.9.7 1.9v2.8c0 .3.2.6.7.5A10 10 0 0 0 12 2Z"/></svg>`],
    [/youtube\.com|youtu\.be/, `<svg viewBox="0 0 24 24" fill="currentColor"><path d="M23 7.2a3 3 0 0 0-2.1-2.1C19 4.6 12 4.6 12 4.6s-7 0-8.9.5A3 3 0 0 0 1 7.2 31 31 0 0 0 .5 12a31 31 0 0 0 .5 4.8 3 3 0 0 0 2.1 2.1c1.9.5 8.9.5 8.9.5s7 0 8.9-.5a3 3 0 0 0 2.1-2.1 31 31 0 0 0 .5-4.8 31 31 0 0 0-.5-4.8ZM9.8 15V9l5.7 3-5.7 3Z"/></svg>`],
    [/twitch\.tv/, `<svg viewBox="0 0 24 24" fill="currentColor"><path d="M4 2 2 6v14h5v3h3l3-3h4l5-5V2H4Zm16 12-3 3h-5l-3 3v-3H5V4h15v10Zm-4-7h-2v5h2V7Zm-5 0H9v5h2V7Z"/></svg>`],
    [/steamcommunity\.com|store\.steampowered/, `<svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 2a10 10 0 0 0-10 9.2l5.4 2.2a2.8 2.8 0 0 1 1.8-.5l2.4-3.5v-.1a3.8 3.8 0 1 1 3.8 3.8h-.1l-3.4 2.5v.2a2.8 2.8 0 0 1-5.6.4L2.4 14.4A10 10 0 1 0 12 2Z"/></svg>`],
    [/t\.me|telegram/, `<svg viewBox="0 0 24 24" fill="currentColor"><path d="M21.9 4.3 18.6 20c-.2 1.1-.9 1.4-1.8.9l-5-3.7-2.4 2.3c-.3.3-.5.5-1 .5l.4-5.1 9.2-8.3c.4-.4-.1-.6-.6-.2L6 13.6 1.1 12c-1.1-.3-1.1-1.1.2-1.6L20.5 3c.9-.3 1.7.2 1.4 1.3Z"/></svg>`],
    [/discord\.(gg|com)/, `<svg viewBox="0 0 24 24" fill="currentColor"><path d="M20.3 4.4A19.8 19.8 0 0 0 15.4 3l-.6 1.3a18.3 18.3 0 0 0-5.6 0L8.6 3a19.7 19.7 0 0 0-4.9 1.4C.6 9 0 13.5.3 18a19.9 19.9 0 0 0 6 3l1.3-2a12.9 12.9 0 0 1-2-1l.5-.4a14.2 14.2 0 0 0 12.2 0l.5.4c-.6.4-1.3.7-2 1l1.3 2a19.8 19.8 0 0 0 6-3c.5-5.2-.9-9.7-3.8-13.6ZM8.5 15.3c-1.2 0-2.2-1.1-2.2-2.4s1-2.4 2.2-2.4 2.2 1.1 2.2 2.4-1 2.4-2.2 2.4Zm7 0c-1.2 0-2.2-1.1-2.2-2.4s1-2.4 2.2-2.4 2.2 1.1 2.2 2.4-1 2.4-2.2 2.4Z"/></svg>`],
  ];
  function linkIcon(url: string): string {
    for (const [re, svg] of LINK_ICONS) if (re.test(url)) return svg;
    return `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="10"/><path d="M2 12h20M12 2a15 15 0 0 1 0 20M12 2a15 15 0 0 0 0 20"/></svg>`;
  }
  function asLinks(v: unknown): { label: string; url: string }[] {
    return Array.isArray(v) ? v.filter((l) => l && typeof l.url === "string" && /^https?:\/\//i.test(l.url)).slice(0, 8).map((l) => ({ label: String(l.label ?? "").slice(0, 40), url: String(l.url) })) : [];
  }
  function asGames(v: unknown): string[] {
    return Array.isArray(v) ? v.filter((g) => typeof g === "string" && g.trim()).slice(0, 12).map((g) => g.slice(0, 60)) : [];
  }
  function asPlayed(v: unknown): { n: string; h: number }[] {
    return Array.isArray(v) ? v.filter((g) => g && typeof g.n === "string").slice(0, 12).map((g) => ({ n: String(g.n).slice(0, 60), h: Number(g.h) || 0 })) : [];
  }
  // Link / game editing in the edit form.
  function setLink(i: number, field: "label" | "url", value: string) {
    if (!draft) return;
    const links = [...(draft.links ?? [])];
    links[i] = { ...(links[i] ?? { label: "", url: "" }), [field]: value };
    draft = { ...draft, links };
  }
  function addLink() { if (draft && (draft.links ?? []).length < 8) draft = { ...draft, links: [...(draft.links ?? []), { label: "", url: "https://" }] }; }
  function removeLink(i: number) { if (draft) draft = { ...draft, links: (draft.links ?? []).filter((_, j) => j !== i) }; }
  let gameInput = $state("");
  function addGame() {
    const g = gameInput.trim();
    if (!draft || !g || (draft.games ?? []).includes(g) || (draft.games ?? []).length >= 12) return;
    draft = { ...draft, games: [...(draft.games ?? []), g] };
    gameInput = "";
  }
  function removeGame(g: string) { if (draft) draft = { ...draft, games: (draft.games ?? []).filter((x) => x !== g) }; }

  async function logOut() {
    try {
      info.set(await api.logOut());
      registrationDismissed.set(false);
      closeProfile();
      openRegistration();
      showToast(tr("Logged out"));
    } catch (e) {
      showToast(`Logout failed: ${e}`);
    }
  }

  let uploading = $state<"" | "avatar" | "banner" | "music">("");

  /** Favourite track: an audio file on your public profile. */
  async function pickMusic() {
    try {
      const result = await openFileDialog({
        multiple: false, directory: false, title: tr("Pick your favourite track"),
        filters: [{ name: "Audio", extensions: ["mp3", "ogg", "opus", "m4a", "aac", "flac", "wav"] }],
      });
      const path = Array.isArray(result) ? (result[0] ?? null) : result;
      if (!path) return;
      uploading = "music";
      await setMyMediaFile("music", path);
      showToast(tr("Favourite track set — visible to everyone"));
    } catch (e) {
      showToast(tr("Could not upload the track: {e}", { e: String(e) }), 6000);
    } finally {
      uploading = "";
    }
  }
  async function removeMusic() {
    try { uploading = "music"; await clearMyMusic(); }
    catch (e) { showToast(String(e)); }
    finally { uploading = ""; }
  }
  const isVideoUrl = (u: string) => /\.(webm|mp4)(\?|$)/i.test(u);

  /** Avatar / banner go straight to the server so everyone sees them. */
  async function pickImage(kind: "avatar" | "banner") {
    try {
      const result = await openFileDialog({
        multiple: false, directory: false, title: kind === "avatar" ? "Pick an avatar" : "Pick a banner",
        filters: [{ name: kind === "banner" ? "Images, GIFs, short videos" : "Images", extensions: kind === "banner" ? ["png", "jpg", "jpeg", "webp", "gif", "mp4", "webm", "mov"] : ["png", "jpg", "jpeg", "webp", "gif"] }],
      });
      const path = Array.isArray(result) ? (result[0] ?? null) : result;
      if (!path) return;
      if (kind === "banner" && /\.(mp4|webm|mov|gif)$/i.test(path)) {
        // Animated banner: re-encoded silent and light (≤ 3 MB).
        uploading = kind;
        await setMyMediaFile("banner", path);
      } else if (path.toLowerCase().endsWith(".gif")) {
        // Animated: upload as-is (cropping would flatten it to one frame).
        uploading = kind;
        await setMyImage(kind, path);
      } else {
        const raw = await api.readUserImage(path);
        const src = URL.createObjectURL(new Blob([new Uint8Array(raw)]));
        const blob = await cropImage(src, kind === "avatar"
          ? { aspect: 1, round: true, outWidth: 512, title: "Position your avatar" }
          : { aspect: 3, outWidth: 1500, title: "Position your banner" });
        URL.revokeObjectURL(src);
        if (!blob) return;
        uploading = kind;
        await setMyImageBytes(kind, "image/webp", Array.from(new Uint8Array(await blob.arrayBuffer())));
      }
      if (draft) draft = { ...draft, avatar_url: $profile.avatar_url, banner_url: $profile.banner_url };
      showToast(kind === "avatar" ? "Avatar updated — visible to everyone" : "Banner updated — visible to everyone");
    } catch (e) {
      showToast(`Could not upload ${kind}: ${e}`, 5000);
    } finally {
      uploading = "";
    }
  }
  async function removeImage(kind: "avatar" | "banner") {
    try {
      uploading = kind;
      await clearMyImage(kind);
      if (draft) draft = { ...draft, avatar_url: $profile.avatar_url, banner_url: $profile.banner_url };
    } catch (e) {
      showToast(`Could not remove ${kind}: ${e}`);
    } finally {
      uploading = "";
    }
  }
  // Draft state for the editor.
  let draft = $state<Profile | null>(null);
  let editing = $state(false);
  let activeUsername = $derived($profileState ?? "");
  let isMe = $derived(activeUsername === "__me__");

  // Build the display view for a contact OR the current user.
  interface View {
    display_name: string;
    username: string;
    about: string;
    phone: string;
    birthday: string;
    favourite_music: string;
    banner_a: string;
    banner_b: string;
    verified: boolean;
    fingerprint: string | null;
    avatar_url: string;
    status: string;
    pronouns: string;
    is_bot: boolean;
    banner_url: string;
    online: string;
    activity: Activity[];
    /** Favourite track (audio file) + its title. */
    music_url: string;
    music_title: string;
    links: { label: string; url: string }[];
    games: string[];
    played: { n: string; h: number }[];
  }
  let view = $derived<View | null>((void $played, buildView(activeUsername, $profile, $contacts, $info, $peerCards, $peerPresence, $myActivity)));
  // Which avatar image to show: the live draft when editing myself, else
  // the stored/served one.
  let shownAvatar = $derived(
    (editing && isMe && draft) ? (draft.avatar_url ?? "") : (view?.avatar_url ?? ""),
  );
  let shownBanner = $derived(
    (editing && isMe && draft) ? (draft.banner_url ?? "") : (view?.banner_url ?? ""),
  );
  let bannerA = $derived((editing && isMe && draft) ? draft.banner_a : (view?.banner_a ?? "#4F7AFE"));
  let bannerB = $derived((editing && isMe && draft) ? draft.banner_b : (view?.banner_b ?? "#9B6CFF"));
  // Now-playing to show on the card: our own when viewing self, else the
  // peer's last-broadcast presence.
  let np = $derived.by(() => {
    if (isMe) return $profile.listening && $profile.listening.title ? $profile.listening : null;
    const p = $peerPresence[activeUsername];
    return p?.listening && p.listening.title ? p.listening : null;
  });

  function buildView(
    uname: string,
    me: Profile,
    _contactsMap: Record<string, unknown>,
    i: typeof $info,
    cards: typeof $peerCards,
    pres: typeof $peerPresence,
    myAct: Activity[],
  ): View | null {
    if (!uname) return null;
    if (uname === "__me__") {
      return {
        display_name: me.display_name,
        username: i?.username ?? "—",
        about: me.about,
        phone: me.phone,
        birthday: me.birthday,
        favourite_music: me.favourite_music,
        banner_a: me.banner_a,
        banner_b: me.banner_b,
        verified: true,
        fingerprint: i?.session_fingerprint ?? null,
        avatar_url: me.avatar_url ?? "",
        status: me.status ?? "",
        pronouns: me.pronouns ?? "",
        is_bot: false,
        banner_url: me.banner_url ?? "",
        online: getStore(myOnline),
        activity: getStore(shareActivity) ? myAct : [],
        music_url: me.music_ver ? (me.music_url ?? "") : "",
        music_title: me.music_title ?? "",
        links: asLinks(me.links),
        games: asGames(me.games),
        played: getStore(shareActivity) ? playedTop(12).map((g) => ({ n: g.name, h: Math.round(g.secs / 360) / 10 })) : [],
      };
    }
    const c = getContact(uname);
    const srv = cards[uname];
    const x = srv?.extra ?? {};
    const pr = pres[uname];
    return {
      // Prefer the server-published card, then the local contact book.
      display_name: c?.full_name || srv?.display_name || uname,
      username: uname,
      // Fields they keep private arrive over E2EE presence (`pp`).
      about: srv?.about || pr?.pp?.about || "",
      phone: srv?.phone || pr?.pp?.phone || c?.phone || "",
      birthday: str(x.birthday) || pr?.pp?.birthday || "",
      favourite_music: str(x.favourite_music),
      banner_a: str(x.banner_a) || (srv?.is_bot ? "#FFB067" : "#4F7AFE"),
      banner_b: str(x.banner_b) || (srv?.is_bot ? "#FF7CAC" : "#9B6CFF"),
      verified: !!srv?.fetched && !!srv?.display_name,
      fingerprint: null,
      avatar_url: srv?.avatar ?? "",
      status: pr?.status || str(x.status),
      pronouns: str(x.pronouns),
      is_bot: !!srv?.is_bot,
      banner_url: srv?.banner ?? "",
      online: pr?.online ?? "",
      activity: pr?.activity ?? [],
      music_url: srv?.music ?? "",
      music_title: str(x.music_title),
      links: asLinks(x.links),
      games: asGames(x.games),
      played: asPlayed(x.played),
    };
  }

  // Refresh the peer's public card whenever their profile opens.
  $effect(() => {
    const u = $profileState;
    privPage = false;
    if (u && u !== "__me__") void refreshPeer(u, true);
  });

  // ---- Privacy for this one person (Telegram X) ----
  let privPage = $state(false);
  const privCount = $derived(activeUsername && !isMe ? exceptionCount(activeUsername, $privacy) : 0);
  function setPriv(k: PrivKey, v: Exception) {
    setException(k, activeUsername, v);
  }
  function setAllPriv(v: Exception) {
    for (const it of PRIV_ITEMS) setException(it.key, activeUsername, v);
  }

  function startEdit() {
    if (!isMe) return;
    draft = { ...$profile };
    editing = true;
  }
  function cancelEdit() { editing = false; draft = null; }
  async function saveEdit() {
    if (!draft) return;
    updateProfile(draft);
    editing = false;
    draft = null;
    // Publish name / bio / banner colours to the server, then tell peers.
    try { await publishMyProfile(); showToast(tr("Profile saved")); }
    catch (e) { broadcastPresence(); showToast(`Saved on this device — will publish when connected (${e})`, 5000); }
  }
  function setListening(field: "title" | "artist" | "url", value: string) {
    if (!draft) return;
    const cur = draft.listening ?? { title: "", artist: "" };
    draft = { ...draft, listening: { ...cur, [field]: value } };
  }

  // Edit contact (saves to contact book).
  let contactDraft = $state<{ full_name: string; phone: string } | null>(null);
  function startContactEdit() {
    if (isMe) return;
    const c = getContact(activeUsername);
    contactDraft = { full_name: c?.full_name ?? "", phone: c?.phone ?? "" };
    editing = true;
  }
  function saveContactEdit() {
    if (!contactDraft) return;
    saveContact(activeUsername, contactDraft);
    editing = false;
    contactDraft = null;
  }

  function onBackdrop(e: MouseEvent) {
    if ((e.target as HTMLElement).classList.contains("backdrop")) closeProfile();
  }

  onMount(() => {
    const h = (e: KeyboardEvent) => { if (e.key === "Escape") closeProfile(); };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  });
</script>

{#if $profileState && view}
  <div
    class="backdrop"
    role="button"
    tabindex="-1"
    aria-label={$t("Close profile")}
    onclick={onBackdrop}
    onkeydown={(e) => { if (e.key === "Escape" || e.key === "Enter") closeProfile(); }}
  >
    <div class="sheet" class:wide={!editing && !privPage} role="dialog" aria-modal="true" tabindex="-1">
      <!-- Close stays put; banner + body scroll together, so the avatar is
           never clipped under the banner when the form is long. -->
      <button class="close" onclick={closeProfile} title={$t("Close (Esc)")}>×</button>
      <div class="scroll">
      <div class="banner" style:background={shownBanner && !isVideoUrl(shownBanner) ? `center / cover no-repeat url("${shownBanner}")` : `linear-gradient(135deg, ${bannerA}, ${bannerB})`}>
        {#if shownBanner && isVideoUrl(shownBanner)}
          <!-- Animated banner: silent short loop. -->
          <video class="banner-vid" use:streamSrc={shownBanner} muted loop autoplay playsinline></video>
        {/if}
        {#if isMe && !editing}
          <button class="banner-hit" onclick={() => pickImage("banner")} disabled={!!uploading} aria-label={$t("Change banner")}>
            <span>{uploading === "banner" ? $t("Uploading…") : $t("📷 Change banner — photo, GIF or video")}</span>
          </button>
        {/if}
        {#if editing && isMe && draft}
          <div class="banner-tools">
            <button class="bn-btn" onclick={() => pickImage("banner")} disabled={!!uploading}>{uploading === "banner" ? "Uploading…" : shownBanner ? "Change banner" : "Upload banner"}</button>
            {#if shownBanner}<button class="bn-btn" onclick={() => removeImage("banner")} disabled={!!uploading}>{$t("Remove")}</button>{/if}
          </div>
        {/if}
      </div>

      <div class="body">
        <div class="avatar-wrap">
          <div class="avatar" style={`background: linear-gradient(135deg, ${bannerA}, ${bannerB});`}>
            {#if shownAvatar}
              <img src={shownAvatar} alt={view.display_name} />
            {:else}
              {view.display_name.charAt(0).toUpperCase()}
            {/if}
          </div>
          {#if view.online && view.online !== "offline"}<span class="presence {view.online}" title={view.online}></span>{/if}
          {#if editing && isMe && draft}
            <button class="av-edit" onclick={() => pickImage("avatar")} title={$t("Change avatar")} disabled={!!uploading}>{$t(uploading === "avatar" ? "…" : "Change")}</button>
            {#if draft.avatar_url}
              <button class="av-remove" onclick={() => removeImage("avatar")} title={$t("Remove avatar")} disabled={!!uploading}>×</button>
            {/if}
          {/if}
        </div>

        {#if editing && isMe && draft}
          <input
            class="edit-name"
            bind:value={draft.display_name}
            placeholder={$t("Display name")}
          />
          <div class="username">@{view.username}</div>
          <div class="pub-note">{$t("Avatar and banner are published to the server — everyone you chat with sees them.")}</div>
          <label class="field">
            <span>{$t("Custom status")}</span>
            <input bind:value={draft.status} placeholder={$t("What's on your mind?")} />
          </label>
          <label class="field">
            <span>{$t("Pronouns")}</span>
            <input bind:value={draft.pronouns} placeholder="she/her · he/him · они" />
          </label>
          <div class="field">
            <span>{$t("Now playing (shown to people you chat with)")}</span>
            <div class="np-edit">
              <input placeholder={$t("Track title")} value={draft.listening?.title ?? ""} oninput={(e) => setListening("title", (e.target as HTMLInputElement).value)} />
              <input placeholder={$t("Artist")} value={draft.listening?.artist ?? ""} oninput={(e) => setListening("artist", (e.target as HTMLInputElement).value)} />
              <input placeholder={$t("Link (Spotify / YouTube, optional)")} value={draft.listening?.url ?? ""} oninput={(e) => setListening("url", (e.target as HTMLInputElement).value)} />
            </div>
          </div>
          <label class="field">
            <span>{$t("About")}</span>
            <textarea bind:value={draft.about} rows="3" placeholder={$t("Tell people a little about yourself...")}></textarea>
          </label>
          <label class="field">
            <span>{$t("Phone")}</span>
            <input bind:value={draft.phone} placeholder="+7 999 123-45-67" />
          </label>
          <label class="field">
            <span>{$t("Birthday")}</span>
            <input bind:value={draft.birthday} placeholder={$t("15 March 1998")} />
          </label>
          <div class="field">
            <span>{$t("Links")}</span>
            {#each draft.links ?? [] as l, i (i)}
              <div class="link-edit">
                <input class="le-label" value={l.label} placeholder={$t("Label")} oninput={(e) => setLink(i, "label", (e.target as HTMLInputElement).value)} />
                <input class="le-url" value={l.url} placeholder="https://…" oninput={(e) => setLink(i, "url", (e.target as HTMLInputElement).value)} />
                <button class="btn-ghost small" onclick={() => removeLink(i)} title={$t("Remove")}>×</button>
              </div>
            {/each}
            {#if (draft.links ?? []).length < 8}<button class="btn-ghost small add-row" onclick={addLink}>+ {$t("Add link")}</button>{/if}
          </div>
          <div class="field">
            <span>{$t("Favourite games")}</span>
            <div class="game-chips">
              {#each draft.games ?? [] as g (g)}<span class="game-chip" style:background={gameGradient(g)}>{g}<button onclick={() => removeGame(g)} title={$t("Remove")}>×</button></span>{/each}
            </div>
            <div class="link-edit">
              <input class="le-url" bind:value={gameInput} placeholder={$t("Teardown, Minecraft…")} onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); addGame(); } }} />
              <button class="btn-ghost small" onclick={addGame}>{$t("Add")}</button>
            </div>
          </div>
          <label class="field">
            <span>{$t("Favourite music")}</span>
            <input bind:value={draft.favourite_music} placeholder={$t("Pink Floyd, 2 Pac, Radiohead...")} />
          </label>
          <div class="field">
            <span>{$t("Favourite track (file)")}</span>
            <div class="track-row">
              <span class="track-name">{$profile.music_ver ? ($profile.music_title || $t("Track")) : $t("No track yet — up to 10 MB")}</span>
              <button class="btn-ghost small" onclick={pickMusic} disabled={!!uploading}>{uploading === "music" ? $t("Uploading…") : $profile.music_ver ? $t("Change") : $t("Upload")}</button>
              {#if $profile.music_ver}<button class="btn-ghost small" onclick={removeMusic} disabled={!!uploading}>{$t("Remove")}</button>{/if}
            </div>
          </div>
          <div class="row">
            <label class="field half">
              <span>{$t("Banner A")}</span>
              <ColorInput bind:value={draft.banner_a} size={36} />
            </label>
            <label class="field half">
              <span>{$t("Banner B")}</span>
              <ColorInput bind:value={draft.banner_b} size={36} />
            </label>
          </div>
          <div class="buttons">
            <button class="btn-ghost" onclick={cancelEdit}>{$t("Cancel")}</button>
            <button class="btn-primary" onclick={saveEdit}>{$t("Save")}</button>
          </div>
        {:else if editing && !isMe && contactDraft}
          <div class="username">@{view.username}</div>
          <label class="field">
            <span>{$t("Full name")}</span>
            <input bind:value={contactDraft.full_name} placeholder="Иван Иванов" />
          </label>
          <label class="field">
            <span>{$t("Phone")}</span>
            <input bind:value={contactDraft.phone} placeholder="+7 999 123-45-67" />
          </label>
          <div class="buttons">
            <button class="btn-ghost" onclick={cancelEdit}>{$t("Cancel")}</button>
            <button class="btn-primary" onclick={saveContactEdit}>{$t("Save")}</button>
          </div>
        {:else if privPage && !isMe}
          <button class="pv-back" onclick={() => (privPage = false)}>‹ {view.display_name}</button>
          <h2 class="name">{$t("Privacy for @{u}", { u: activeUsername })}</h2>
          <div class="pv-sub">
            {$t("Rules here override your general privacy settings for this person and show up there as exceptions.")}
            {#if $contactSet.has(activeUsername)}<b>{$t("In your contacts.")}</b>{/if}
          </div>
          <div class="pv-quick">
            <button onclick={() => setAllPriv("allow")}>{$t("Allow everything")}</button>
            <button onclick={() => setAllPriv("deny")}>{$t("Restrict everything")}</button>
            <button onclick={() => setAllPriv(null)}>{$t("Reset")}</button>
          </div>
          {#each PRIV_ITEMS as it (it.key)}
            {@const ex = exceptionOf(it.key, activeUsername, $privacy)}
            {@const eff = allowed(it.key, activeUsername, $privacy)}
            <div class="pv-row">
              <div class="pv-text">
                <div class="pv-name">{$t(it.label)}</div>
                <div class="pv-eff" class:no={!eff}>{eff ? $t("Can see / can do") : $t("Restricted")}{#if !ex}{" · "}{$t("by default")}{/if}</div>
              </div>
              <div class="pv-seg">
                <button class:on={ex === null} onclick={() => setPriv(it.key, null)} title={$t("Follow general settings")}>{$t("Default")}</button>
                <button class:on={ex === "allow"} class="ok" onclick={() => setPriv(it.key, "allow")}>{$t("Allow")}</button>
                <button class:on={ex === "deny"} class="bad" onclick={() => setPriv(it.key, "deny")}>{$t("Deny")}</button>
              </div>
            </div>
          {/each}
        {:else}
        <div class="pf-grid">
        <div class="pf-left">
          <h2 class="name">
            {view.display_name}
            {#if view.verified}<span class="badge" title={$t("Verified identity")}>{@html icons.verified}</span>{/if}
            {#if view.is_bot}<span class="bot-tag">BOT</span>{/if}
            {#if view.pronouns}<span class="pronouns">{view.pronouns}</span>{/if}
            {#if $clans[isMe ? ($info?.username ?? "") : activeUsername]}<ClanTag tag={$clans[isMe ? ($info?.username ?? "") : activeUsername]} size="md" />{/if}
          </h2>
          <div class="username">@{view.username}</div>
          {#if view.status}
            <div class="status-line">{view.status}</div>
          {/if}

          {#if view.about}
            <section class="section">
              <div class="section-label">{$t("About me")}</div>
              <p class="section-body">{view.about}</p>
            </section>
          {/if}

          {#if view.links.length}
            <section class="section">
              <div class="section-label">{$t("Links")}</div>
              <div class="links">
                {#each view.links as l, i (i)}
                  <button class="link-row" onclick={() => api.openExternal(l.url).catch(() => {})} title={l.url}>
                    <span class="link-ic">{@html linkIcon(l.url)}</span>
                    <span class="link-txt">{l.label || hostOf(l.url)}</span>
                    <span class="link-go">↗</span>
                  </button>
                {/each}
              </div>
            </section>
          {/if}

          {#if view.phone}
            <section class="section">
              <div class="section-label">{$t("Phone")}</div>
              <p class="section-body mono">{view.phone}</p>
            </section>
          {/if}

          {#if view.birthday}
            <section class="section">
              <div class="section-label">{$t("Birthday")}</div>
              <p class="section-body">{view.birthday}</p>
            </section>
          {/if}

          {#if view.fingerprint}
            <section class="section">
              <div class="section-label">{$t("Session fingerprint")}</div>
              <p class="section-body mono small">{view.fingerprint}</p>
            </section>
          {/if}

          <div class="buttons">
            {#if isMe}
              <button class="btn-primary" onclick={startEdit}>{$t("Edit my profile")}</button>
              <button class="btn-ghost danger" onclick={logOut}>{$t("Log out")}</button>
            {:else}
              <button class="btn-ghost danger" onclick={() => setBlocked(activeUsername, !$blocked.has(activeUsername))}>
                {$blocked.has(activeUsername) ? $t("Unblock") : $t("Block")}
              </button>
              <button class="btn-ghost" onclick={() => (privPage = true)}>
                {$t("Privacy for this person")}{#if privCount > 0} <span class="pv-badge">{privCount}</span>{/if}
              </button>
              <button class="btn-primary" onclick={startContactEdit}>{$t("Edit contact")}</button>
            {/if}
          </div>
        </div>

        <div class="pf-right">
          <div class="pf-tabs" role="tablist">
            <button role="tab" class:on={pfTab === "board"} aria-selected={pfTab === "board"} onclick={() => (pfTab = "board")}>{$t("Board")}</button>
            <button role="tab" class:on={pfTab === "activity"} aria-selected={pfTab === "activity"} onclick={() => (pfTab = "activity")}>{$t("Activity")}</button>
          </div>

          {#if pfTab === "board"}
            {#each view.activity.filter((a, i, all) => all.findIndex((b) => b.kind + b.name === a.kind + a.name) === i) as a (a.kind + a.name)}
              <div class="act-card">
                <div class="act-ic" data-k={a.kind}>{a.kind === "playing" ? "🎮" : a.kind === "listening" ? "🎧" : "📺"}</div>
                <div class="np-info">
                  <div class="np-label">{activityVerb(a)}</div>
                  <div class="np-track"><b>{a.name}</b></div>
                  {#if activityLine(a)}<div class="act-line">{activityLine(a)}</div>{/if}
                </div>
              </div>
            {/each}

            {#if np}
              <a class="np-card" href={np.url || undefined} target="_blank" rel="noopener" title={np.url ? "Open" : ""}>
                <span class="np-ic">♪</span>
                <div class="np-info">
                  <div class="np-label">{$t("Listening to")}</div>
                  <div class="np-track">{np.title}{#if np.artist} · {np.artist}{/if}</div>
                </div>
              </a>
            {/if}

            {#if view.music_url}
              <section class="card">
                <div class="section-label">{$t("Favourite track")}</div>
                <div class="fav-track">
                  <MediaPlayer src={view.music_url} kind="audio" name={view.music_title || undefined} />
                  {#if view.music_title}<div class="fav-title">{view.music_title}</div>{/if}
                </div>
              </section>
            {/if}

            {#if view.games.length || view.played.length}
              <section class="card">
                <div class="section-label">{$t("Game collection")}</div>
                <div class="games">
                  {#each [...new Set([...view.games, ...view.played.map((g) => g.n)])].slice(0, 12) as g (g)}
                    {@const pl = view.played.find((x) => x.n === g)}
                    <div class="game" style:background={gameGradient(g)} title={g}>
                      <span class="game-name">{g}</span>
                      {#if pl && pl.h > 0}<span class="game-h">{pl.h} {$t("h")}</span>{/if}
                      {#if view.games.includes(g)}<span class="game-fav" title={$t("Favourite")}>★</span>{/if}
                    </div>
                  {/each}
                </div>
              </section>
            {/if}

            {#if view.favourite_music}
              <section class="card">
                <div class="section-label">{$t("Favourite music")}</div>
                <p class="section-body">{view.favourite_music}</p>
              </section>
            {/if}

            {#if !view.activity.length && !np && !view.music_url && !view.games.length && !view.played.length && !view.favourite_music}
              <div class="pf-empty">{isMe ? $t("Add a favourite track, games and links in “Edit profile”.") : $t("Nothing here yet.")}</div>
            {/if}
          {:else}
            {#each view.activity as a (a.kind + a.name)}
              <div class="act-card">
                <div class="act-ic" data-k={a.kind}>{a.kind === "playing" ? "🎮" : a.kind === "listening" ? "🎧" : "📺"}</div>
                <div class="np-info">
                  <div class="np-label">{$t("Now")} · {activityVerb(a)}</div>
                  <div class="np-track"><b>{a.name}</b></div>
                  {#if activityLine(a)}<div class="act-line">{activityLine(a)}</div>{/if}
                </div>
              </div>
            {/each}
            {#if view.played.length}
              <div class="section-label" style="margin-top:10px">{$t("Played recently")}</div>
              {#each view.played as g (g.n)}
                <div class="played-row">
                  <span class="played-cover" style:background={gameGradient(g.n)}>{g.n.charAt(0).toUpperCase()}</span>
                  <span class="played-name">{g.n}</span>
                  <span class="played-h">{g.h > 0 ? `${g.h} ${$t("h")}` : $t("< 1 h")}</span>
                </div>
              {/each}
            {:else if !view.activity.length}
              <div class="pf-empty">{isMe ? $t("Games you play show up here automatically (with activity sharing on).") : $t("No activity to show.")}</div>
            {/if}
          {/if}
        </div>
        </div>
        {/if}
      </div>
      </div>
    </div>
  </div>
{/if}

<style>
  /* Wide, two-column profile (view mode) */
  .sheet.wide { width: min(880px, 96vw) !important; }
  .pf-grid { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.15fr); gap: 22px; align-items: start; }
  .pf-left { min-width: 0; }
  .pf-right { min-width: 0; display: flex; flex-direction: column; gap: 10px; }
  @media (max-width: 760px) { .pf-grid { grid-template-columns: 1fr; } }
  .pf-tabs { display: flex; gap: 18px; border-bottom: 1px solid var(--border); margin-bottom: 4px; }
  .pf-tabs button {
    border: none; background: none; padding: 8px 0; cursor: pointer; font-size: 13.5px; font-weight: 600;
    color: var(--fg-dim); border-bottom: 2px solid transparent; margin-bottom: -1px;
    transition: color 0.15s, border-color 0.15s;
  }
  .pf-tabs button.on { color: var(--fg); border-bottom-color: var(--accent); }
  .card { padding: 12px; border-radius: 12px; background: var(--bg-input); border: 1px solid var(--border); }
  .pf-empty { font-size: 12.5px; color: var(--fg-dim); padding: 18px 4px; text-align: center; }
  .links { display: flex; flex-direction: column; gap: 2px; }
  .link-row {
    display: flex; align-items: center; gap: 10px; padding: 7px 8px; border: none; border-radius: 8px;
    background: none; color: var(--fg); cursor: pointer; text-align: left; font-size: 13px;
    transition: background 0.12s;
  }
  .link-row:hover { background: var(--bg-input); }
  .link-ic { width: 18px; height: 18px; flex-shrink: 0; color: var(--fg-muted); display: grid; place-items: center; }
  .link-ic :global(svg) { width: 18px; height: 18px; }
  .link-txt { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .link-go { color: var(--fg-dim); font-size: 12px; }
  .games { display: grid; grid-template-columns: repeat(auto-fill, minmax(96px, 1fr)); gap: 8px; margin-top: 8px; }
  .game {
    position: relative; aspect-ratio: 3 / 4; border-radius: 10px; padding: 8px;
    display: flex; flex-direction: column; justify-content: flex-end; overflow: hidden;
    color: #fff; box-shadow: inset 0 -40px 40px -20px rgba(0, 0, 0, 0.55);
    transition: transform 0.15s ease;
  }
  .game:hover { transform: translateY(-2px); }
  .game-name { font-size: 12px; font-weight: 700; line-height: 1.2; text-shadow: 0 1px 2px rgba(0, 0, 0, 0.6); }
  .game-h { font-size: 10.5px; opacity: 0.85; }
  .game-fav { position: absolute; top: 6px; right: 7px; font-size: 12px; color: #FFD45C; text-shadow: 0 1px 2px rgba(0, 0, 0, 0.5); }
  .played-row { display: flex; align-items: center; gap: 10px; padding: 6px 2px; font-size: 13px; }
  .played-cover { width: 34px; height: 34px; border-radius: 8px; display: grid; place-items: center; color: #fff; font-weight: 800; flex-shrink: 0; }
  .played-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .played-h { color: var(--fg-dim); font-size: 12px; }
  .link-edit { display: flex; gap: 6px; margin-top: 6px; }
  .link-edit input { min-width: 0; }
  .le-label { flex: 0 0 34%; }
  .le-url { flex: 1; }
  .add-row { margin-top: 6px; align-self: flex-start; }
  .game-chips { display: flex; flex-wrap: wrap; gap: 6px; }
  .game-chip { display: inline-flex; align-items: center; gap: 4px; padding: 3px 4px 3px 10px; border-radius: 999px; color: #fff; font-size: 12px; font-weight: 600; }
  .game-chip button { border: none; background: rgba(0, 0, 0, 0.25); color: #fff; width: 18px; height: 18px; border-radius: 50%; cursor: pointer; line-height: 1; }

  .backdrop {
    position: fixed; inset: 0;
    background: rgba(6, 7, 12, 0.6);
    backdrop-filter: blur(4px);
    display: grid; place-items: center;
    z-index: 120;
    animation: fade-in 0.15s ease-out;
    padding: 20px;
  }
  @keyframes fade-in { from { opacity: 0; } to { opacity: 1; } }

  .sheet {
    width: min(440px, 94vw);
    max-height: 86vh;
    display: flex; flex-direction: column;
    background: var(--bg-shell);
    border: 1px solid var(--border);
    border-radius: 16px;
    box-shadow: 0 32px 80px -16px rgba(0, 0, 0, 0.6);
    overflow: hidden;
    position: relative;
    animation: slide-up 0.22s ease-out;
  }
  .scroll { flex: 1 1 auto; min-height: 0; overflow-y: auto; overscroll-behavior: contain; }
  @keyframes slide-up {
    from { opacity: 0; transform: translateY(12px); }
    to   { opacity: 1; transform: translateY(0); }
  }

  .banner {
    height: 120px;
    position: relative;
    flex-shrink: 0;
    overflow: hidden;
  }
  .banner-vid { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; pointer-events: none; }
  .track-row { display: flex; align-items: center; gap: 8px; }
  .track-name { flex: 1; min-width: 0; font-size: 13px; color: var(--fg-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .btn-ghost.small { padding: 5px 10px; font-size: 12px; }
  .fav-track { display: flex; flex-direction: column; gap: 4px; }
  .fav-title { font-size: 12.5px; color: var(--fg-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .banner-hit { position: absolute; inset: 0; border: none; background: rgba(0,0,0,0); color: #fff; cursor: pointer;
    display: grid; place-items: center; font: inherit; font-weight: 700; font-size: 13px; transition: background .2s; }
  .banner-hit span { opacity: 0; transform: translateY(4px); transition: opacity .2s, transform .2s; background: rgba(0,0,0,.55);
    padding: 6px 12px; border-radius: 10px; backdrop-filter: blur(6px); }
  .banner-hit:hover { background: rgba(0,0,0,.25); }
  .banner-hit:hover span, .banner-hit:focus-visible span { opacity: 1; transform: none; }
  .banner-tools { position: absolute; right: 12px; bottom: 12px; display: flex; gap: 6px; align-items: center; }
  .bn-btn {
    background: rgba(0, 0, 0, 0.55); color: #fff; border: none; border-radius: 8px;
    padding: 5px 10px; font-size: 12px; font-weight: 700; cursor: pointer; backdrop-filter: blur(6px);
  }
  .bn-btn:hover:not(:disabled) { background: rgba(0, 0, 0, 0.75); }
  .bn-btn:disabled { opacity: 0.6; cursor: default; }
  .pv-back { background: none; border: none; color: var(--accent); font: inherit; font-weight: 700; font-size: 13px; padding: 0 0 6px; cursor: pointer; }
  .pv-sub { font-size: 12.5px; color: var(--fg-dim); line-height: 1.5; margin: 6px 0 12px; }
  .pv-sub b { color: var(--fg-muted); font-weight: 600; margin-left: 4px; }
  .pv-quick { display: flex; gap: 6px; flex-wrap: wrap; margin-bottom: 10px; }
  .pv-quick button {
    border: 1px solid var(--border); background: var(--bg-input); color: var(--fg-muted); border-radius: 100px;
    padding: 5px 11px; font: inherit; font-size: 12px; font-weight: 600; cursor: pointer;
  }
  .pv-quick button:hover { color: var(--accent); border-color: var(--accent-soft); }
  .pv-row { display: flex; align-items: center; gap: 10px; padding: 9px 12px; background: var(--bg-input); border-radius: 10px; margin-bottom: 6px; }
  .pv-text { flex: 1; min-width: 0; }
  .pv-name { font-size: 13.5px; font-weight: 600; color: var(--fg); }
  .pv-eff { font-size: 11.5px; color: var(--accent); }
  .pv-eff.no { color: #ff8f8f; }
  .pv-seg { display: flex; background: var(--bg-shell); border: 1px solid var(--border); border-radius: 9px; padding: 2px; flex-shrink: 0; }
  .pv-seg button { border: none; background: none; color: var(--fg-dim); font: inherit; font-size: 11.5px; font-weight: 700; padding: 5px 8px; border-radius: 7px; cursor: pointer; }
  .pv-seg button.on { background: var(--bg-hover); color: var(--fg); }
  .pv-seg button.ok.on { background: color-mix(in srgb, var(--accent) 18%, transparent); color: var(--accent); }
  .pv-seg button.bad.on { background: rgba(255, 77, 109, 0.16); color: #ff8f8f; }
  .pv-badge { background: var(--accent); color: #06070C; border-radius: 100px; padding: 0 6px; font-size: 11px; margin-left: 4px; }
  .pub-note { font-size: 11.5px; color: var(--fg-dim); margin: 4px 0 10px; }
  .act-card {
    display: flex; align-items: center; gap: 12px;
    margin-top: 12px; padding: 10px 12px;
    background: var(--bg-input); border: 1px solid var(--border); border-radius: 12px;
  }
  .act-ic { width: 40px; height: 40px; border-radius: 10px; display: grid; place-items: center; font-size: 20px; background: var(--accent-soft); flex-shrink: 0; }
  .act-line { font-size: 12px; color: var(--fg-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .close {
    position: absolute;
    z-index: 3;
    top: 10px; right: 10px;
    width: 28px; height: 28px;
    background: rgba(0, 0, 0, 0.35);
    border: none;
    border-radius: 8px;
    color: #fff;
    font-size: 18px;
    line-height: 1;
    cursor: pointer;
    backdrop-filter: blur(6px);
  }
  .close:hover { background: rgba(0, 0, 0, 0.55); }

  .body {
    position: relative;
    padding: 0 22px 22px;
    margin-top: -44px;
  }

  .avatar-wrap { position: relative; width: 88px; margin-bottom: 14px; }
  .avatar {
    width: 88px; height: 88px;
    border-radius: 50%;
    display: grid; place-items: center;
    font-size: 36px;
    font-weight: 800;
    color: #06070C;
    border: 5px solid var(--bg-shell);
    box-shadow: 0 8px 20px -4px rgba(0, 0, 0, 0.4);
    overflow: hidden;
  }
  .avatar img { width: 100%; height: 100%; object-fit: cover; }
  .presence {
    position: absolute;
    right: 4px; bottom: 4px;
    width: 18px; height: 18px;
    border-radius: 50%;
    background: var(--online);
    border: 4px solid var(--bg-shell);
  }
  .presence.idle { background: #FAA61A; }
  .presence.dnd { background: #ED4245; }
  .av-edit {
    position: absolute;
    left: 50%; bottom: -6px; transform: translateX(-50%);
    padding: 3px 10px;
    background: var(--accent); color: #06070C;
    border: 2px solid var(--bg-shell); border-radius: 100px;
    font-size: 10.5px; font-weight: 700; cursor: pointer;
    white-space: nowrap;
  }
  .av-edit:hover { filter: brightness(1.08); }
  .av-remove {
    position: absolute;
    right: -2px; top: -2px;
    width: 22px; height: 22px;
    background: var(--danger); color: #fff;
    border: 2px solid var(--bg-shell); border-radius: 50%;
    font-size: 13px; line-height: 1; cursor: pointer;
  }

  .bot-tag {
    font-size: 9.5px; font-weight: 800; letter-spacing: 0.06em;
    background: var(--accent); color: #06070C;
    padding: 2px 6px; border-radius: 5px;
  }
  .pronouns {
    font-size: 11.5px; font-weight: 600; color: var(--fg-dim);
    background: var(--bg-input); border: 1px solid var(--border);
    padding: 2px 8px; border-radius: 100px;
  }
  .status-line {
    font-size: 13px; color: var(--fg-muted);
    margin: -10px 0 16px; line-height: 1.4;
  }

  .np-card {
    display: flex; align-items: center; gap: 11px;
    padding: 10px 12px; margin-bottom: 14px;
    background: var(--accent-soft); border: 1px solid var(--accent-soft);
    border-radius: 12px; text-decoration: none;
  }
  .np-card:hover { border-color: var(--accent); }
  .np-ic {
    width: 34px; height: 34px; flex-shrink: 0; border-radius: 9px;
    display: grid; place-items: center; font-size: 17px;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070C;
  }
  .np-info { min-width: 0; }
  .np-label { font-size: 10.5px; font-weight: 700; color: var(--accent); text-transform: uppercase; letter-spacing: 0.06em; }
  .np-track { font-size: 13px; color: var(--fg); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .np-edit { display: flex; flex-direction: column; gap: 6px; }

  .name {
    margin: 0;
    font-size: 20px;
    font-weight: 800;
    letter-spacing: -0.01em;
    display: flex; align-items: center; gap: 6px;
  }
  .badge :global(svg) { width: 16px; height: 16px; color: var(--accent); filter: drop-shadow(0 0 4px var(--accent-glow)); }
  .username {
    font-family: var(--font-mono);
    font-size: 12.5px;
    color: var(--fg-dim);
    margin: 2px 0 16px;
  }

  .section {
    background: var(--bg-input);
    border-radius: 10px;
    padding: 10px 14px;
    margin-bottom: 8px;
  }
  .section-label {
    font-size: 11px;
    font-weight: 700;
    color: var(--fg-dim);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    margin-bottom: 4px;
  }
  .section-body {
    margin: 0;
    font-size: 13.5px;
    color: var(--fg);
    line-height: 1.45;
    white-space: pre-wrap;
    overflow-wrap: break-word;
  }
  .section-body.mono { font-family: var(--font-mono); font-size: 12px; color: var(--fg-muted); }
  .section-body.small { font-size: 11px; }

  .edit-name {
    width: 100%;
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--border-strong);
    color: var(--fg);
    font-family: inherit;
    font-size: 20px;
    font-weight: 800;
    padding: 2px 0 6px;
    outline: none;
  }
  .edit-name:focus { border-bottom-color: var(--accent); }

  .field { display: flex; flex-direction: column; gap: 4px; margin-top: 10px; }
  .field span {
    font-size: 11px;
    font-weight: 700;
    color: var(--fg-dim);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }
  .field input, .field textarea {
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--fg);
    font-family: inherit;
    font-size: 13.5px;
    padding: 8px 11px;
    outline: none;
    resize: vertical;
  }
  .field input:focus, .field textarea:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .field input[type="color"] { padding: 2px; height: 36px; cursor: pointer; }
  .row { display: flex; gap: 10px; }
  .field.half { flex: 1; }

  .buttons {
    display: flex; justify-content: flex-end; gap: 10px;
    margin-top: 16px;
  }
  .btn-ghost, .btn-primary {
    padding: 8px 16px;
    border-radius: 10px;
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
    border: none;
    font-family: inherit;
  }
  .btn-ghost {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--fg-dim);
  }
  .btn-ghost:hover { color: var(--fg); border-color: var(--border-strong); }
  .btn-ghost.danger { color: #ff8f8f; }
  .btn-ghost.danger:hover { color: #ff6b6b; border-color: rgba(255,107,107,0.4); background: rgba(255,77,109,0.08); }
  .btn-primary {
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070C;
    box-shadow: 0 6px 16px -4px var(--accent-glow);
  }
  .btn-primary:hover { transform: translateY(-1px); }
</style>
