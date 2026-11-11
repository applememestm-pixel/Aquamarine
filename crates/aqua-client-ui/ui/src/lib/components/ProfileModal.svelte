<script module lang="ts">
  import { writable, get as getStore } from "svelte/store";

  // Null closed, string = username whose profile we're showing,
  // or "__me__" for the current user.
  export const profileState = writable<string | null>(null);

  import { info as me } from "../stores";
  // Our own name (a reply, a forward, a mention of us) opens our own profile.
  export function openProfile(username = "__me__") {
    const mine = getStore(me)?.username;
    profileState.set(mine && username.replace(/^@/, "").toLowerCase() === mine.toLowerCase() ? "__me__" : username);
  }
  export function closeProfile() { profileState.set(null); }
</script>

<script lang="ts">
  import { openLightbox } from "./Lightbox.svelte";
  import UsernameEdit from "./UsernameEdit.svelte";
  import BirthdayInput from "./BirthdayInput.svelte";
  import ColorInput from "./ColorInput.svelte";
  import { onMount } from "svelte";
  import { profile, updateProfile, type Profile } from "../profile";
  import { contacts, saveContact, getContact } from "../contacts";
  import { info, chats } from "../stores";
  import { api, bytesToDataUrl } from "../tauri";
  import { icons } from "../icons";
  import { openFileDialog } from "../dialog";
  import { showToast } from "./Toast.svelte";
  import { broadcastPresence, peerPresence, myActivity, shareActivity, myOnline, activityVerb, activityLine, peerStatus } from "../presence";
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
  import GamePoster from "./GamePoster.svelte";
  import QrCode from "./QrCode.svelte";
  import EmojiStatus from "./EmojiStatus.svelte";
  import { reactionPack, emojiPacks, loadEmojiPacks } from "../reactionPack";
  import SharedMedia from "./SharedMedia.svelte";
  import OfficialMark from "./OfficialMark.svelte";
  import { NAME_COLORS } from "../people";
  import { averageColor, mixHex, patternRingUrl, PATTERN_EMOJI, cardLook, shellColor, CARD_SWATCHES, CARD_INTENSITY, CARD_LENGTH } from "../cardStyle";
  import EmojiPick from "./EmojiPick.svelte";
  import { userQr } from "../qr";
  import { gameIds, gameIcons, gameCovers, resolveGame, rememberGame, iconFor, coverFor } from "../games";
  import { SERVICES, serviceOf, type Service } from "../services";

  let pfTab = $state<"board" | "activity" | "media">("board");
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
    return Array.isArray(v) ? v.filter((l) => l && typeof l.url === "string" && /^https?:\/\//i.test(l.url)).slice(0, 12).map((l) => ({ label: String(l.label ?? "").slice(0, 40), url: String(l.url) })) : [];
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
  function addLink() { if (draft && (draft.links ?? []).length < 12) draft = { ...draft, links: [...(draft.links ?? []), { label: "", url: "https://" }] }; }
  function removeLink(i: number) { if (draft) draft = { ...draft, links: (draft.links ?? []).filter((_, j) => j !== i) }; }
  // Connections (lib/services.ts): a service + a name there → a profile link.
  let connPick = $state(false);
  let connSvc = $state<Service | null>(null);
  let connHandle = $state("");
  function addConn() {
    const hd = connHandle.trim().replace(/^@/, "");
    if (!draft || !connSvc || !hd || (draft.links ?? []).length >= 12) return;
    draft = { ...draft, links: [...(draft.links ?? []), { label: connSvc.name, url: connSvc.url(hd) }] };
    connSvc = null; connHandle = "";
  }
  /** Readable text on a service's colour. */
  function onColor(hex: string): string {
    const n = parseInt(hex.slice(1), 16);
    const l = (0.299 * (n >> 16) + 0.587 * ((n >> 8) & 255) + 0.114 * (n & 255)) / 255;
    return l > 0.6 ? "#111" : "#fff";
  }
  let gameInput = $state("");
  let gameHits = $state<{ id: string; name: string; has_icon: boolean }[]>([]);
  let gameTimer: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    const q = gameInput.trim();
    if (gameTimer) clearTimeout(gameTimer);
    if (q.length < 2) { gameHits = []; return; }
    gameTimer = setTimeout(() => {
      void api.gamesSearch(q).then((h) => { if (gameInput.trim() === q) { gameHits = h.slice(0, 8); h.slice(0, 8).forEach((x) => x.has_icon && resolveGame(x.name, x.id)); } }).catch(() => (gameHits = []));
    }, 250);
  });
  function addGame(hit?: { id: string; name: string }) {
    const g = (hit?.name ?? gameInput).trim();
    if (!draft || !g || (draft.games ?? []).includes(g) || (draft.games ?? []).length >= 12) return;
    const ids = { ...(draft.game_ids ?? {}) };
    if (hit) { ids[g] = hit.id; rememberGame(g, hit.id); }
    draft = { ...draft, games: [...(draft.games ?? []), g], game_ids: ids };
    resolveGame(g, hit?.id);
    gameInput = ""; gameHits = [];
  }
  function makeFavourite(g: string) { if (draft) draft = { ...draft, games: [g, ...(draft.games ?? []).filter((x) => x !== g)] }; }
  let showAllGames = $state(false);
  function removeGame(g: string) { if (draft) { const ids = { ...(draft.game_ids ?? {}) }; delete ids[g]; draft = { ...draft, games: (draft.games ?? []).filter((x) => x !== g), game_ids: ids }; } }
  // Look up icons for whatever is on screen.
  $effect(() => {
    const v = view;
    if (!v) return;
    for (const g of v.games) resolveGame(g, v.game_ids[g]);
    for (const p of v.played) resolveGame(p.n);
  });

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
      // Kept locally and uploaded on the next connection (people.ts).
      if (draft) draft = { ...draft, avatar_url: $profile.avatar_url, banner_url: $profile.banner_url };
      showToast(`Could not upload ${kind}: ${e}`, 6000);
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
  /** The account's ID (never changes, unlike the @name). */
  let uid = $derived(isMe ? ($info?.uid ?? "") : ($peerCards[activeUsername]?.uid ?? ""));

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
    game_ids: Record<string, string>;
    game_note: string;
    played: { n: string; h: number }[];
    /** Card colour set by hand ("" = from the banner) and banner emoji. */
    card: string;
    /** How strong the card colour is / how far down it fades (0–100). */
    card_i: number;
    card_g: number;
    pattern: string;
    estatus: string;
  }
  let view = $derived<View | null>((void $played, buildView(activeUsername, $profile, $contacts, $info, $peerCards, $peerPresence, $myActivity)));
  // Links to a known service show as connections, the rest as links.
  const conns = $derived((view?.links ?? []).map((l) => ({ url: l.url, ...(serviceOf(l.url) ?? { s: null, handle: "" }) })).filter((c) => !!c.s) as { url: string; s: Service; handle: string }[]);
  const plainLinks = $derived((view?.links ?? []).filter((l) => !serviceOf(l.url)));
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
  // Card colour: set by hand, else the banner picture's average colour,
  // else halfway between the two banner colours.
  let sampled = $state("");
  $effect(() => {
    const u = shownBanner && !isVideoUrl(shownBanner) ? shownBanner : "";
    sampled = "";
    if (u) void averageColor(u).then((c) => { if (u === shownBanner) sampled = c; });
  });
  let cardSet = $derived((editing && isMe && draft) ? (draft.card ?? "") : (view?.card ?? ""));
  let cardColor = $derived(cardSet || sampled || mixHex(bannerA, bannerB));
  let cardI = $derived((editing && isMe && draft) ? (draft.card_i ?? CARD_INTENSITY) : (view?.card_i ?? CARD_INTENSITY));
  let cardG = $derived((editing && isMe && draft) ? (draft.card_g ?? CARD_LENGTH) : (view?.card_g ?? CARD_LENGTH));
  const shell = shellColor();
  /** Background of the whole card and whether its text must be dark. */
  let look = $derived(cardLook(cardColor, cardI, cardG, shell));
  let esOpen = $state(false);
  let patOpen = $state(false);
  let patternEmoji = $derived((editing && isMe && draft) ? (draft.pattern ?? "") : (view?.pattern ?? ""));
  let patternBg = $derived(patternEmoji ? patternRingUrl(patternEmoji) : "");
  let showQr = $state(false);

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
        game_ids: me.game_ids ?? {},
        game_note: (me.game_note ?? "").slice(0, 140),
        played: getStore(shareActivity) ? playedTop(12).map((g) => ({ n: g.name, h: Math.round(g.secs / 360) / 10 })) : [],
        card: me.card ?? "",
        card_i: me.card_i ?? CARD_INTENSITY,
        card_g: me.card_g ?? CARD_LENGTH,
        pattern: me.pattern ?? "",
        estatus: me.estatus ?? "",
      };
    }
    const c = getContact(uname);
    const srv = cards[uname];
    const x = srv?.extra ?? {};
    const pr = pres[uname];
    return {
      // Prefer the server-published card, then the local contact book.
      display_name: c?.full_name || (srv?.display_name && srv.display_name !== "You" ? srv.display_name : "")
        || (getStore(chats).find((ch) => ch.id === uname)?.name ?? "") || uname,
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
      online: peerStatus(pr),
      activity: peerStatus(pr) === "offline" ? [] : (pr?.activity ?? []),
      music_url: srv?.music ?? "",
      music_title: str(x.music_title),
      links: asLinks(x.links),
      games: asGames(x.games),
      game_ids: (x.game_ids && typeof x.game_ids === "object" ? x.game_ids : {}) as Record<string, string>,
      game_note: str(x.game_note).slice(0, 140),
      played: asPlayed(x.played),
      card: /^#[0-9a-f]{6}$/i.test(str(x.card)) ? str(x.card) : "",
      card_i: typeof x.ci === "number" ? x.ci : CARD_INTENSITY,
      card_g: typeof x.cg === "number" ? x.cg : CARD_LENGTH,
      pattern: str(x.pattern).slice(0, 16),
      estatus: str(x.es),
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
    void loadEmojiPacks();
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
    onkeydown={(e) => { if (e.target === e.currentTarget && (e.key === "Escape" || e.key === "Enter")) closeProfile(); }}
  >
    <div class="sheet" data-phone-full class:wide={!editing && !privPage} class:ed={editing && isMe && !!draft} class:light={look.light} style:--card={cardColor} style:--card-bg={look.background} style:--card-base={look.base} style:--card-top={look.top} style:--card-line={look.border} role="dialog" aria-modal="true" tabindex="-1">
      <!-- Close stays put; banner + body scroll together, so the avatar is
           never clipped under the banner when the form is long. -->
      <button data-x class="close" onclick={closeProfile} title={$t("Close (Esc)")}>×</button>
      <!-- Phones: the profile is a whole screen and × is hidden there — an
           explicit "back" like Telegram's, so it can always be left. -->
      <button class="p-back" onclick={closeProfile} aria-label={$t("Back")}>{@html icons.back}</button>
      <div class="scroll">
      <div class="banner" style:background={shownBanner && !isVideoUrl(shownBanner) ? `center / cover no-repeat url("${shownBanner}")` : `linear-gradient(135deg, ${bannerA}, ${bannerB})`}>
        {#if shownBanner && isVideoUrl(shownBanner)}
          <!-- Animated banner: silent short loop. -->
          <video class="banner-vid" use:streamSrc={shownBanner} muted loop autoplay playsinline></video>
        {/if}
        {#if patternBg}<div class="pattern" style:background-image={`url("${patternBg}")`}></div>{/if}
        {#if isMe && !editing}
          <button class="banner-hit" onclick={() => pickImage("banner")} disabled={!!uploading} aria-label={$t("Change banner")}>
            <span>{uploading === "banner" ? $t("Uploading…") : $t("📷 Change banner — photo, GIF or video")}</span>
          </button>
        {/if}
        {#if editing && isMe && draft}
          <div class="banner-tools">
            <button class="bn-btn" onclick={() => pickImage("banner")} disabled={!!uploading}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 8h3l2-3h6l2 3h3v11H4z"/><circle cx="12" cy="13" r="3.5"/></svg>
              {uploading === "banner" ? $t("Uploading…") : shownBanner ? $t("Change banner") : $t("Upload banner")}</button>
            {#if shownBanner}<button class="bn-btn icon" onclick={() => removeImage("banner")} disabled={!!uploading} title={$t("Remove the banner")} aria-label={$t("Remove the banner")}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/></svg></button>{/if}
          </div>
        {/if}
      </div>

      <div class="body">
        <div class="avatar-wrap">
          <div class="avatar" style={`background: linear-gradient(135deg, ${bannerA}, ${bannerB});`}>
            {#if shownAvatar}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
              <img src={shownAvatar} alt={view.display_name} class:zoomable={!editing}
                onclick={() => { if (!editing && shownAvatar) openLightbox({ url: shownAvatar, kind: /\.(mp4|webm)(\?|$)/i.test(shownAvatar) ? "video" : "image", name: view?.display_name, from: view?.display_name }); }} />
            {:else}
              {view.display_name.charAt(0).toUpperCase()}
            {/if}
          </div>
          {#if view.online}<span class="presence {view.online}" title={view.online}></span>{/if}
          {#if editing && isMe && draft}
            <button class="av-edit" onclick={() => pickImage("avatar")} title={$t("Change avatar")} aria-label={$t("Change avatar")} disabled={!!uploading}>
              {#if uploading === "avatar"}…{:else}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 8h3l2-3h6l2 3h3v11H4z"/><circle cx="12" cy="13" r="3.5"/></svg>{/if}</button>
            {#if draft.avatar_url}
              <button class="av-remove" onclick={() => removeImage("avatar")} title={$t("Remove avatar")} disabled={!!uploading}>×</button>
            {/if}
          {/if}
        </div>

        {#if editing && isMe && draft}
          <div class="ed-left">
          <input
            class="edit-name"
            bind:value={draft.display_name}
            placeholder={$t("Display name")}
          />
          <div class="field">
            <span>{$t("Username")}</span>
            <UsernameEdit />
          </div>
          <label class="field">
            <span>{$t("Custom status")}</span>
            <input bind:value={draft.status} placeholder={$t("What's on your mind?")} />
          </label>
          <label class="field">
            <span>{$t("About")}</span>
            <textarea bind:value={draft.about} rows="4" placeholder={$t("Tell people a little about yourself...")}></textarea>
          </label>
          <div class="field">
            <span>{$t("Emoji next to your name")}</span>
            <div class="es-row">
              <button class="es-cur" onclick={() => (esOpen = !esOpen)} title={$t("Choose an emoji")} aria-expanded={esOpen}>
                {#if draft.estatus}<EmojiStatus token={draft.estatus} size={30} />{:else}<span class="es-none">+</span>{/if}
              </button>
              <div class="es-txt">
                <button class="lnk" onclick={() => (esOpen = !esOpen)}>{draft.estatus ? $t("Change") : $t("Choose an emoji")}</button>
                {#if draft.estatus}<button class="lnk dim" onclick={() => draft && (draft = { ...draft, estatus: "" })}>{$t("Remove")}</button>{/if}
              </div>
              {#if esOpen}<EmojiPick value={draft.estatus} onpick={(tok) => draft && (draft = { ...draft, estatus: tok })} onclose={() => (esOpen = false)} />{/if}
            </div>
            <small class="es-note">{$t("Any emoji, or one from your emoji and sticker packs. A tap on it shows it big.")}</small>
          </div>
          <div class="field">
            <span>{$t("Card colour")}</span>
            <div class="sw-grid">
              <button class="sw auto" class:on={!draft.card} onclick={() => draft && (draft = { ...draft, card: "" })} title={$t("From the banner")}
                style:background={sampled || mixHex(draft.banner_a, draft.banner_b)}><span>{$t("Auto")}</span></button>
              {#each CARD_SWATCHES as c (c)}
                <button class="sw" class:on={draft.card?.toLowerCase() === c.toLowerCase()} style:background={c} onclick={() => draft && (draft = { ...draft, card: c })} aria-label={c}></button>
              {/each}
              <span class="sw-own" title={$t("Any colour")}><ColorInput value={draft.card || cardColor} size={30} oninput={(v) => draft && (draft = { ...draft, card: v })} /></span>
            </div>
            <label class="slider"><span>{$t("Intensity")}</span>
              <input type="range" min="10" max="100" value={draft.card_i ?? CARD_INTENSITY} oninput={(e) => draft && (draft = { ...draft, card_i: +(e.target as HTMLInputElement).value })} />
              <b>{draft.card_i ?? CARD_INTENSITY}%</b></label>
            <label class="slider"><span>{$t("Gradient length")}</span>
              <input type="range" min="0" max="100" value={draft.card_g ?? CARD_LENGTH} oninput={(e) => draft && (draft = { ...draft, card_g: +(e.target as HTMLInputElement).value })} />
              <b>{(draft.card_g ?? CARD_LENGTH) === 0 ? $t("solid") : `${draft.card_g ?? CARD_LENGTH}%`}</b></label>
          </div>
          <div class="field">
            <span>{$t("Name and reply colour")}</span>
            <div class="nc-row">
              <button class="nc" class:on={!draft.name_color} onclick={() => draft && (draft = { ...draft, name_color: "" })} title={$t("Automatic")} aria-label={$t("Automatic")}>A</button>
              {#each NAME_COLORS as c (c)}
                <button class="nc" class:on={draft.name_color?.toLowerCase() === c.toLowerCase()} style:--nc={c} onclick={() => draft && (draft = { ...draft, name_color: c })} aria-label={c}></button>
              {/each}
            </div>
            <div class="nc-prev" style:--nc={draft.name_color || "var(--accent)"}>
              <b>{draft.display_name || $t("You")}</b>
              <span class="nc-q"><b>{draft.display_name || $t("You")}</b><i>{$t("Reply to your message looks like this")}</i></span>
            </div>
          </div>
          <div class="field">
            <span>{$t("Banner pattern")}</span>
            <div class="pat-grid">
              <button class="pat" class:on={!draft.pattern} onclick={() => draft && (draft = { ...draft, pattern: "" })} title={$t("No pattern")}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="8"/><path d="M6.5 17.5l11-11"/></svg></button>
              {#each PATTERN_EMOJI as e (e)}
                <button class="pat" class:on={draft.pattern === e} onclick={() => draft && (draft = { ...draft, pattern: e })}>{e}</button>
              {/each}
              <span class="pat-own-wrap">
                <button class="pat own" class:on={!!draft.pattern && !PATTERN_EMOJI.includes(draft.pattern)} onclick={() => (patOpen = !patOpen)} title={$t("Any emoji")}>
                  {#if draft.pattern && !PATTERN_EMOJI.includes(draft.pattern)}{draft.pattern}{:else}+{/if}</button>
                {#if patOpen}<EmojiPick plain value={draft.pattern} onpick={(tok) => draft && (draft = { ...draft, pattern: tok })} onclose={() => (patOpen = false)} />{/if}
              </span>
            </div>
          </div>
          {#if !shownBanner}
            <div class="field">
              <span>{$t("Banner colours (without a picture)")}</span>
              <div class="row bn-cols">
                <ColorInput bind:value={draft.banner_a} size={30} />
                <span class="bn-arrow">→</span>
                <ColorInput bind:value={draft.banner_b} size={30} />
              </div>
            </div>
          {/if}
          <div class="pub-note">{$t("Avatar and banner are published to the server — everyone you chat with sees them.")}</div>
          </div>
          <div class="ed-right">
          <div class="row">
            <label class="field half">
              <span>{$t("Phone")}</span>
              <input bind:value={draft.phone} placeholder="+7 999 123-45-67" />
            </label>
            <div class="field half">
              <span>{$t("Birthday")}</span>
              <BirthdayInput bind:value={draft.birthday} />
            </div>
          </div>
          <div class="field">
            <span>{$t("Links")}</span>
            {#each draft.links ?? [] as l, i (i)}
              <div class="link-edit">
                <input class="le-label" value={l.label} placeholder={$t("Label")} oninput={(e) => setLink(i, "label", (e.target as HTMLInputElement).value)} />
                <input class="le-url" value={l.url} placeholder="https://…" oninput={(e) => setLink(i, "url", (e.target as HTMLInputElement).value)} />
                <button class="btn-ghost small" onclick={() => removeLink(i)} title={$t("Remove")}>×</button>
              </div>
            {/each}
            {#if (draft.links ?? []).length < 12}<button class="btn-ghost small add-row" onclick={addLink}>+ {$t("Add link")}</button>{/if}
          </div>
          <div class="field">
            <span>{$t("Connections")}</span>
            {#if connSvc}
              <div class="link-edit">
                <span class="svc-dot" style:background={connSvc.color} style:color={onColor(connSvc.color)}>{connSvc.name.charAt(0)}</span>
                <input class="le-url" bind:value={connHandle} placeholder={tr("Your name on {s}", { s: connSvc.name })} autocomplete="off" spellcheck="false"
                  onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); addConn(); } if (e.key === "Escape") connSvc = null; }} />
                <button class="btn-ghost small" onclick={addConn} disabled={!connHandle.trim()}>{$t("Add")}</button>
                <button class="btn-ghost small" onclick={() => (connSvc = null)} title={$t("Cancel")}>×</button>
              </div>
            {:else if connPick}
              <div class="svc-grid">
                {#each SERVICES as sv (sv.id)}
                  <button class="svc" onclick={() => { connSvc = sv; connPick = false; connHandle = ""; }}>
                    <span class="svc-dot" style:background={sv.color} style:color={onColor(sv.color)}>{sv.name.charAt(0)}</span>{sv.name}
                  </button>
                {/each}
              </div>
            {:else if (draft.links ?? []).length < 12}
              <button class="btn-ghost small add-row" onclick={() => (connPick = true)}>+ {$t("Connect a service")}</button>
            {/if}
          </div>
          <div class="field">
            <span>{$t("Favourite games")}</span>
            {#if (draft.games ?? []).length}
              <div class="ed-games">
                {#each draft.games ?? [] as g (g)}
                  {@const cv = coverFor(g, $gameIds, $gameCovers, draft.game_ids)}
                  {@const ic = iconFor(g, $gameIds, $gameIcons, draft.game_ids)}
                  <div class="ed-game" class:first={g === draft.games?.[0]}>
                    <GamePoster name={g} cover={cv} icon={ic} />
                    {#if g !== draft.games?.[0]}<button class="eg-star" onclick={() => makeFavourite(g)} title={$t("Make it my favourite game")} aria-label={$t("Make it my favourite game")}>★</button>{:else}<span class="eg-first">★ {$t("Favourite")}</span>{/if}
                    <button class="eg-x" onclick={() => removeGame(g)} title={$t("Remove")} aria-label={$t("Remove")}>×</button>
                  </div>
                {/each}
              </div>
            {/if}
            {#if (draft.games ?? []).length}
              <input class="game-note" bind:value={draft.game_note} maxlength="140" placeholder={tr("What do you love about {g}?", { g: draft.games?.[0] ?? "" })} />
            {/if}
            <div class="link-edit game-search">
              <input class="le-url" bind:value={gameInput} placeholder={$t("Search a game — Teardown, Minecraft…")} onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); addGame(gameHits[0] && gameHits[0].name.toLowerCase() === gameInput.trim().toLowerCase() ? gameHits[0] : undefined); } }} />
              <button class="btn-ghost small" onclick={() => addGame()}>{$t("Add")}</button>
              {#if gameHits.length}
                <div class="game-hits">
                  {#each gameHits as h (h.id)}
                    {@const ic = $gameIcons[h.id]}
                    <button class="game-hit" onclick={() => addGame(h)}>
                      {#if ic}<img src={ic} alt="" />{:else}<span class="gh-ph" style:background={gameGradient(h.name)}>{h.name.charAt(0)}</span>{/if}
                      <span>{h.name}</span>
                    </button>
                  {/each}
                </div>
              {/if}
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
          <div class="buttons">
            <button class="btn-ghost" onclick={cancelEdit}>{$t("Cancel")}</button>
            <button class="btn-primary" onclick={saveEdit}>{$t("Save")}</button>
          </div>
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
            {view.display_name}{#if !isMe && $peerCards[activeUsername]?.verified}<OfficialMark size={22} />{/if}<EmojiStatus token={view.estatus} size={24} zoom />
            {#if view.verified}<span class="badge" title={$t("Verified identity")}>{@html icons.verified}</span>{/if}
            {#if view.is_bot}<span class="bot-tag">BOT</span>{/if}

            {#if $clans[isMe ? ($info?.username ?? "") : activeUsername]}<ClanTag tag={$clans[isMe ? ($info?.username ?? "") : activeUsername]} size="md" />{/if}
          </h2>
          <div class="username">@{view.username}
            {#if !view.is_bot}<button class="qr-btn" onclick={() => (showQr = true)} title={$t("QR code")} aria-label={$t("QR code")}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><path d="M14 14h3v3M21 14v.01M17 21h4v-4M14 18v3"/></svg>
            </button>{/if}
          </div>
          {#if uid}
            <button class="uid-chip" title={$t("Account ID — it never changes, even when the @name does. Tap to copy.")}
              onclick={() => { void navigator.clipboard?.writeText(uid); showToast(tr("ID copied")); }}>ID {uid}</button>
          {/if}
          {#if showQr}
            <div class="qr-pop" role="presentation" onclick={() => (showQr = false)}>
              <div class="qr-card" role="presentation" onclick={(e) => e.stopPropagation()}>
                <QrCode text={userQr(view.username, isMe ? ($info?.server_url ?? "") : "")} color={cardColor} avatar={shownAvatar} letter={view.display_name.charAt(0).toUpperCase()} />
                <b>@{view.username}</b>
                <small>{$t("Scan it in Aquamarine: New message → Scan QR.")}</small>
              </div>
            </div>
          {/if}
          {#if view.status}
            <div class="status-line">{view.status}</div>
          {/if}

          {#if view.about}
            <section class="section">
              <div class="section-label">{$t("About me")}</div>
              <p class="section-body">{view.about}</p>
            </section>
          {/if}

          {#if conns.length}
            <section class="section">
              <div class="section-label">{$t("Connections")}</div>
              <div class="conns">
                {#each conns as c, i (i)}
                  <button class="conn" onclick={() => api.openExternal(c.url).catch(() => {})} title={c.url}>
                    <span class="svc-dot" style:background={c.s.color} style:color={onColor(c.s.color)}>{c.s.name.charAt(0)}</span>
                    <span class="conn-t"><b>{c.handle}</b><small>{c.s.name}</small></span>
                  </button>
                {/each}
              </div>
            </section>
          {/if}
          {#if plainLinks.length}
            <section class="section">
              <div class="section-label">{$t("Links")}</div>
              <div class="links">
                {#each plainLinks as l, i (i)}
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
            {#if !isMe && activeUsername}
              <!-- Your shared photos, files, voice and links (Telegram's profile tabs). -->
              <button role="tab" class:on={pfTab === "media"} aria-selected={pfTab === "media"} onclick={() => (pfTab = "media")}>{$t("Shared media")}</button>
            {/if}
          </div>
          {#if pfTab === "media" && !isMe && activeUsername}
            <div class="pf-media">{#key activeUsername}<SharedMedia peerId={activeUsername} title={view.display_name} />{/key}</div>
          {/if}

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

            {#if view.games.length}
              {@const fg = view.games[0]}
              {@const fpl = view.played.find((x) => x.n === fg)}
              {@const now = view.activity.some((a) => a.name === fg)}
              <section class="card">
                <div class="section-label">{$t("Favourite game")}</div>
                <div class="fav-game">
                  <div class="fg-poster"><GamePoster name={fg} cover={coverFor(fg, $gameIds, $gameCovers, view.game_ids)} icon={iconFor(fg, $gameIds, $gameIcons, view.game_ids)} label={false} /></div>
                  <div class="fg-info">
                    <b class="fg-name">{fg}</b>
                    {#if view.game_note}<p class="fg-note">“ {view.game_note}</p>{/if}
                    <div class="fg-chips">
                      {#if now}<span class="fg-chip live">● {$t("Playing now")}</span>{/if}
                      {#if fpl && fpl.h > 0}<span class="fg-chip">🕹 {fpl.h} {$t("h")} {$t("in game")}</span>{/if}
                      {#if (view.games.length > 1)}<span class="fg-chip">★ {tr("{n} favourite games", { n: view.games.length })}</span>{/if}
                    </div>
                  </div>
                </div>
              </section>
              {#if view.games.length > 1}
                <section class="card">
                  <div class="section-label">{$t("My favourite games")}</div>
                  <div class="games">
                    <!-- Every starred game, the main one too; the name under the poster
                         (on the poster it was cut off by the card's edge). -->
                    {#each view.games.slice(0, showAllGames ? 16 : 8) as g (g)}
                      {@const pl = view.played.find((x) => x.n === g)}
                      <div class="g-item" title={g}>
                        <GamePoster name={g} bare cover={coverFor(g, $gameIds, $gameCovers, view.game_ids)} icon={iconFor(g, $gameIds, $gameIcons, view.game_ids)} hours={pl?.h ?? 0} />
                        <span class="g-name">{g}</span>
                      </div>
                    {/each}
                  </div>
                  {#if view.games.length > 8}<button class="more-games" onclick={() => (showAllGames = !showAllGames)}>{showAllGames ? $t("Show less") : $t("Show more")}</button>{/if}
                </section>
              {/if}
            {/if}
            {#if view.played.length || view.games.length}
              {@const all = [...new Set([...view.played.map((g) => g.n), ...view.games])]}
              <section class="card">
                <div class="section-label">{$t("All games")} · {all.length}</div>
                {#each all as g (g)}
                  {@const pl = view.played.find((x) => x.n === g)}
                  {@const ic = iconFor(g, $gameIds, $gameIcons, view.game_ids)}
                  {@const max = Math.max(1, ...view.played.map((x) => x.h))}
                  <div class="played-row">
                    {#if ic}<img class="played-cover" src={ic} alt="" />{:else}<span class="played-cover" style:background={gameGradient(g)}>{g.charAt(0).toUpperCase()}</span>{/if}
                    <span class="played-name">{g}{#if view.games.includes(g)} <span class="game-star">★</span>{/if}</span>
                    {#if pl}<span class="played-bar"><span style:width="{Math.max(4, (pl.h / max) * 100)}%"></span></span>{/if}
                    <span class="played-h">{pl ? (pl.h > 0 ? `${pl.h} ${$t("h")}` : $t("< 1 h")) : ""}</span>
                  </div>
                {/each}
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
          {:else if pfTab === "activity"}
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
                {@const ic = iconFor(g.n, $gameIds, $gameIcons)}
                <div class="played-row">
                  {#if ic}<img class="played-cover" src={ic} alt="" />{:else}<span class="played-cover" style:background={gameGradient(g.n)}>{g.n.charAt(0).toUpperCase()}</span>{/if}
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
  .avatar img.zoomable { cursor: zoom-in; }
  /* Wide, two-column profile (view mode) */
  .sheet.wide { width: min(880px, 96vw) !important; }
  .pf-grid { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.15fr); gap: 22px; align-items: start; }
  .pf-left { min-width: 0; }
  .pf-right { min-width: 0; display: flex; flex-direction: column; gap: 10px; }
  .pf-media { min-height: 240px; max-height: 60vh; display: flex; flex-direction: column; border-radius: calc(14px * var(--rk, 1)); overflow: hidden;
    background: color-mix(in srgb, var(--bg-shell) 40%, transparent); border: 1px solid var(--border); }
  :global(html.phone) .pf-media { max-height: none; }
  @media (max-width: 760px) { .pf-grid { grid-template-columns: 1fr; } }
  .pf-tabs { display: flex; gap: 18px; border-bottom: 1px solid var(--border); margin-bottom: 4px; }
  .pf-tabs button {
    border: none; background: none; padding: 8px 0; cursor: pointer; font-size: 13.5px; font-weight: 600;
    color: var(--fg-dim); border-bottom: 2px solid transparent; margin-bottom: -1px;
    transition: color 0.15s, border-color 0.15s;
  }
  .pf-tabs button.on { color: var(--fg); border-bottom-color: var(--accent); }
  .card { padding: 12px; border-radius: calc(12px * var(--rk, 1)); background: var(--bg-input); border: 1px solid var(--border); }
  .pf-empty { font-size: 12.5px; color: var(--fg-dim); padding: 18px 4px; text-align: center; }
  .links { display: flex; flex-direction: column; gap: 2px; }
  /* Connections: a chip per service account. */
  .conns { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 6px; }
  .conn { display: flex; align-items: center; gap: 9px; padding: 7px 9px; border: 1px solid var(--border); border-radius: calc(10px * var(--rk, 1));
    background: var(--bg-input); color: var(--fg); cursor: pointer; text-align: left; min-width: 0; transition: border-color .12s; }
  .conn:hover { border-color: var(--accent); }
  .conn-t { min-width: 0; display: flex; flex-direction: column; line-height: 1.2; }
  .conn-t b, .conn-t small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .conn-t b { font-size: 13px; } .conn-t small { font-size: 11px; color: var(--fg-muted); }
  .svc-dot { width: 24px; height: 24px; flex: none; border-radius: 50%; display: grid; place-items: center; font-size: 12px; font-weight: 800; }
  .svc-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(118px, 1fr)); gap: 6px; }
  .svc { display: flex; align-items: center; gap: 8px; padding: 6px 8px; border: 1px solid var(--border); border-radius: calc(10px * var(--rk, 1));
    background: transparent; color: var(--fg); font: inherit; font-size: 12.5px; cursor: pointer; text-align: left; }
  .svc:hover { border-color: var(--accent); }
  .link-row {
    display: flex; align-items: center; gap: 10px; padding: 7px 8px; border: none; border-radius: calc(8px * var(--rk, 1));
    background: none; color: var(--fg); cursor: pointer; text-align: left; font-size: 13px;
    transition: background 0.12s;
  }
  .link-row:hover { background: var(--bg-input); }
  .link-ic { width: 18px; height: 18px; flex-shrink: 0; color: var(--fg-muted); display: grid; place-items: center; }
  .link-ic :global(svg) { width: 18px; height: 18px; }
  .link-txt { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .link-go { color: var(--fg-dim); font-size: 12px; }
  /* Bigger posters: 3 a row in the profile window, 4 on a phone's full width. */
  .games { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px 10px; margin-top: 8px; }
  :global(html.phone) .games { grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 10px 8px; }
  .g-item { display: flex; flex-direction: column; gap: 5px; min-width: 0; }
  .g-name { font-size: 12px; font-weight: 600; line-height: 1.25; color: var(--fg); text-align: center;
    overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; word-break: break-word; }
  .game-star { color: #FFD45C; font-size: 11px; }
  .more-games { margin-top: 10px; border: none; background: none; color: var(--fg-muted); font: inherit; font-size: 13px; font-weight: 600; cursor: pointer; padding: 0; }
  .more-games:hover { color: var(--accent); text-decoration: underline; }
  .fav-game { display: flex; gap: 14px; margin-top: 8px; align-items: flex-start; }
  .fg-poster { width: 92px; flex-shrink: 0; }
  .fg-info { min-width: 0; display: flex; flex-direction: column; gap: 6px; padding-top: 4px; }
  .fg-name { font-size: 16px; }
  .fg-note { margin: 0; font-size: 13px; color: var(--fg-muted); line-height: 1.45; word-wrap: break-word; }
  .fg-chips { display: flex; flex-wrap: wrap; gap: 6px; }
  .fg-chip { font-size: 11.5px; font-weight: 600; padding: 3px 8px; border-radius: calc(7px * var(--rk, 1)); border: 1px solid var(--border-strong); color: var(--fg-muted); }
  .fg-chip.live { color: #3ddc84; border-color: color-mix(in srgb, #3ddc84 40%, transparent); }

  .p-back { display: none; position: absolute; z-index: 6; top: calc(10px + env(safe-area-inset-top, 0px)); left: 10px; width: 38px; height: 38px;
    place-items: center; border: none; border-radius: 50%; background: rgba(0, 0, 0, .45); color: #fff; cursor: pointer; backdrop-filter: blur(6px); }
  .p-back :global(svg) { width: 22px; height: 22px; }
  /* ---- editor: two columns (card on the left, details on the right) ---- */
  .sheet.ed { width: min(980px, 96vw); height: min(720px, 92vh); max-height: 92vh; }
  .sheet.ed .scroll { display: grid; grid-template-columns: minmax(300px, 360px) 1fr; grid-template-rows: 150px auto minmax(0, 1fr); overflow: hidden; }
  .sheet.ed .banner { grid-column: 1 / span 2; grid-row: 1; height: 150px; }
  .sheet.ed .body { display: contents; }
  .sheet.ed .avatar-wrap { grid-column: 1; grid-row: 2; margin: -52px 0 14px 24px; z-index: 2; }
  .field .track-name { text-transform: none; letter-spacing: normal; font-size: 13px; font-weight: 400; color: var(--fg-muted); }
  /* Both columns start on the same line (under the avatar), same side padding. */
  .ed-left { grid-column: 1; grid-row: 3; padding: 0 24px 18px; overflow-y: auto; min-height: 0; scrollbar-width: thin; scrollbar-gutter: stable; }
  .ed-right { grid-column: 2; grid-row: 3; padding: 0 24px 18px; border-left: 1px solid var(--border); overflow-y: auto; min-height: 0;
    display: flex; flex-direction: column; scrollbar-width: thin; }
  .ed-right .buttons { margin-top: auto; padding-top: 16px; }
  .ed-games { display: grid; grid-template-columns: repeat(auto-fill, minmax(76px, 1fr)); gap: 8px; margin: 2px 0 8px; }
  .ed-game { position: relative; animation: fade-in .2s ease-out; }
  .ed-game.first :global(.gp) { box-shadow: 0 0 0 2px #FFD45C; }
  .eg-star { position: absolute; top: 4px; left: 4px; width: 20px; height: 20px; border-radius: 50%; border: none; cursor: pointer;
    background: rgba(0,0,0,.6); color: #FFD45C; font-size: 12px; line-height: 1; opacity: 0; transition: opacity .15s; }
  .ed-game:hover .eg-star { opacity: 1; }
  .eg-first { position: absolute; top: 4px; left: 4px; white-space: nowrap; text-transform: none; letter-spacing: 0; font-size: 9.5px; font-weight: 800; color: #1a1205; background: #FFD45C; border-radius: calc(6px * var(--rk, 1)); padding: 1px 5px; }
  .game-note { width: 100%; box-sizing: border-box; margin-bottom: 8px; }
  .eg-x { position: absolute; top: 4px; right: 4px; width: 20px; height: 20px; border-radius: 50%; border: none; cursor: pointer;
    background: rgba(0,0,0,.6); color: #fff; font-size: 13px; line-height: 1; opacity: 0; transition: opacity .15s; }
  .ed-game:hover .eg-x, .eg-x:focus-visible { opacity: 1; }
  /* Phones: the profile is a page of its own — the whole screen, no ×
     (Back closes it), like Telegram. */
  @media (max-width: 720px) {
    .backdrop { padding: 0 !important; place-items: stretch; }
    .sheet, .sheet.wide, .sheet.ed { width: 100% !important; max-width: none !important; height: 100% !important; max-height: none !important;
      border-radius: 0; border: none; padding-top: env(safe-area-inset-top, 0px); padding-bottom: env(safe-area-inset-bottom, 0px); }
    .close { display: none; }
    .p-back { display: grid; }
    .sheet:not(.ed) .banner { height: 210px; }
  }
  @media (max-width: 760px) {
    .sheet.ed { height: auto; }
    .sheet.ed .scroll { display: block; overflow-y: auto; }
    .sheet.ed .banner { height: 130px; }
    .sheet.ed .body { display: block; padding: 0 18px 18px; margin-top: -44px; }
    .sheet.ed .avatar-wrap { margin: 0 0 14px; }
    .ed-left, .ed-right { padding: 0; border: none; overflow: visible; }
  }
  .played-bar { flex: 0 0 70px; height: 5px; border-radius: 3px; background: var(--bg-input); overflow: hidden; }
  .played-bar span { display: block; height: 100%; background: var(--accent); border-radius: 3px; }
  img.played-cover { object-fit: cover; }
  .game-chip img { width: 16px; height: 16px; border-radius: calc(4px * var(--rk, 1)); margin-right: 5px; vertical-align: -3px; }
  .game-search { position: relative; }
  .game-hits { position: absolute; left: 0; right: 0; top: calc(100% + 4px); z-index: 20; background: var(--bg-shell); border: 1px solid var(--border-strong);
    border-radius: calc(12px * var(--rk, 1)); padding: 4px; box-shadow: 0 18px 40px -14px rgba(0,0,0,.7); max-height: 260px; overflow-y: auto; }
  .game-hit { width: 100%; display: flex; align-items: center; gap: 10px; padding: 6px 8px; border: none; border-radius: calc(8px * var(--rk, 1)); background: none; color: var(--fg); font: inherit; font-size: 13px; cursor: pointer; text-align: left; }
  .game-hit:hover { background: var(--accent-soft); }
  .game-hit img, .gh-ph { width: 28px; height: 28px; border-radius: calc(7px * var(--rk, 1)); object-fit: cover; flex-shrink: 0; display: grid; place-items: center; color: #fff; font-weight: 800; }
  .game:hover { transform: translateY(-2px); }
  .game-name { font-size: 12px; font-weight: 700; line-height: 1.2; text-shadow: 0 1px 2px rgba(0, 0, 0, 0.6); }
  .game-h { font-size: 10.5px; opacity: 0.85; }
  .game-fav { position: absolute; top: 6px; right: 7px; font-size: 12px; color: #FFD45C; text-shadow: 0 1px 2px rgba(0, 0, 0, 0.5); }
  .played-row { display: flex; align-items: center; gap: 10px; padding: 6px 2px; font-size: 13px; }
  .played-cover { width: 34px; height: 34px; border-radius: calc(8px * var(--rk, 1)); display: grid; place-items: center; color: #fff; font-weight: 800; flex-shrink: 0; }
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

  /* The whole card takes its colour (from the banner unless set by hand). */
  .sheet {
    background: var(--card-bg) !important;
    border-color: color-mix(in srgb, var(--card-line) 40%, transparent) !important;
  }
  /* A pale card: dark text and controls, so everything stays readable. */
  .sheet.light {
    --fg: #15171d; --fg-muted: #383d48; --fg-dim: #5a606d;
    --border: rgba(0, 0, 0, 0.14); --border-strong: rgba(0, 0, 0, 0.24);
    --bg-input: rgba(255, 255, 255, 0.55); --bg-hover: rgba(0, 0, 0, 0.06);
    --bg-shell: #f5f6f8; --accent-soft: rgba(0, 0, 0, 0.07);
    color: var(--fg);
  }
  .sheet.light :global(input), .sheet.light :global(textarea) { color: var(--fg); }
  .sheet.light :global(input::placeholder), .sheet.light :global(textarea::placeholder) { color: var(--fg-dim); }
  .sheet :global(.section), .sheet .card, .sheet .act-card {
    background: color-mix(in srgb, var(--card) 22%, rgba(0, 0, 0, 0.30)) !important;
    border-color: color-mix(in srgb, var(--card) 22%, transparent) !important;
  }
  .sheet .avatar, .sheet .presence { border-color: var(--card-top); }
  /* Rings of the emoji around the avatar (Telegram), clearly visible. */
  .pattern { position: absolute; inset: 0; pointer-events: none; background-size: auto 100%; background-repeat: no-repeat; background-position: left top; opacity: 0.5; mix-blend-mode: overlay; }
  .sheet.light .pattern { mix-blend-mode: normal; opacity: 0.35; filter: invert(1); }
  .field small.es-note { font-size: 11.5px; color: var(--fg-dim); text-transform: none; letter-spacing: 0; font-weight: 400; line-height: 1.4; }
  /* Emoji next to the name */
  .es-row { position: relative; display: flex; align-items: center; gap: 12px; }
  /* One column grid for the whole editor: the emoji box, the colours and the
     patterns are all 1/8 of the width, so their edges line up. */
  .es-cur { width: calc((100% - 42px) / 8); aspect-ratio: 1; height: auto; flex-shrink: 0; border-radius: calc(12px * var(--rk, 1)); border: 1px dashed var(--border-strong); background: var(--bg-input);
    display: grid; place-items: center; cursor: pointer; color: var(--fg-muted); transition: border-color .15s, transform .15s; }
  .es-cur:hover { border-color: var(--accent); transform: scale(1.04); }
  .es-cur :global(.es) { margin: 0; }
  .es-none { font-size: 22px; font-weight: 300; }
  .es-txt { display: flex; align-items: center; gap: 12px; align-self: stretch; }
  .es-txt .lnk { line-height: 1.2; padding: 6px 0; }
  .lnk { border: none; background: none; padding: 0; font: inherit; font-size: 13px; font-weight: 700; color: var(--accent); cursor: pointer; text-transform: none; letter-spacing: 0; }
  .sheet.light .lnk { color: var(--fg); text-decoration: underline; }
  .lnk.dim { color: var(--fg-dim); }
  .lnk:hover { text-decoration: underline; }
  /* Card colour */
  .sw-grid { display: grid; grid-template-columns: repeat(8, 1fr); gap: 6px; align-items: center; justify-items: center; }
  .sw { width: 100%; aspect-ratio: 1; height: auto; border-radius: 50%; border: 2px solid rgba(255, 255, 255, 0.14); cursor: pointer; padding: 0; position: relative;
    transition: transform .12s; box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.18); }
  .sw:hover { transform: scale(1.1); }
  .sw.on { border-color: var(--fg); box-shadow: 0 0 0 2px var(--card-base), 0 0 0 4px var(--fg); }
  /* "Auto" sits on the banner colour, which can be white: a dark chip of its own keeps it readable. */
  .sw.auto { border-color: var(--border-strong); }
  .sw.auto span { position: absolute; inset: 18%; display: grid; place-items: center; border-radius: 50%; background: rgba(10, 12, 18, .62); font-size: 8px; font-weight: 800; color: #fff; text-transform: uppercase; letter-spacing: .02em; }
  .sw-own { display: grid; place-items: center; width: 100%; }
  .sw-own :global(.cp-swatch) { width: 100% !important; height: auto !important; aspect-ratio: 1; border-radius: 50%; }
  /* Label and value on one line, the slider under them at full width. */
  .slider { display: grid; grid-template-columns: 1fr auto; grid-template-areas: "l v" "r r"; align-items: center; column-gap: 10px; row-gap: 4px; margin-top: 12px; text-transform: none; letter-spacing: 0; }
  .slider > span { grid-area: l; }
  .slider > b { grid-area: v; }
  .slider > input { grid-area: r; margin: 0; }
  .field .slider > span { text-transform: none; letter-spacing: 0; font-size: 12.5px; font-weight: 600; color: var(--fg-muted); }
  .slider b { font-size: 12px; color: var(--fg-muted); text-align: right; font-variant-numeric: tabular-nums; }
  .slider input[type="range"] { width: 100%; accent-color: var(--accent); }
  .sheet.light .slider input[type="range"] { accent-color: #15171d; }
  .nc-row { display: flex; flex-wrap: wrap; gap: 8px; }
  .nc { width: 28px; height: 28px; border-radius: 50%; border: none; cursor: pointer; background: var(--nc, var(--bg-input)); color: var(--fg-muted); font: 700 12px var(--font-ui, inherit);
    box-shadow: 0 0 0 2px var(--card-bg, var(--bg-shell)); transition: transform .15s, box-shadow .15s; }
  .nc.on { box-shadow: 0 0 0 2px var(--card-bg, var(--bg-shell)), 0 0 0 4px var(--nc, var(--fg-muted)); transform: scale(1.05); }
  .nc-prev { margin-top: 8px; display: flex; flex-direction: column; gap: 6px; padding: 10px 12px; border-radius: calc(12px * var(--rk, 1)); background: var(--bg-input); }
  .nc-prev > b { color: var(--nc); font-size: 13.5px; }
  .nc-q { display: flex; flex-direction: column; padding: 4px 8px; border-left: 3px solid var(--nc); border-radius: calc(6px * var(--rk, 1)); background: color-mix(in srgb, var(--nc) 14%, transparent); font-size: 12.5px; }
  .nc-q b { color: var(--nc); }
  .nc-q i { font-style: normal; color: var(--fg-muted); }
  /* Banner pattern */
  .pat-grid { display: grid; grid-template-columns: repeat(8, 1fr); gap: 6px; }
  .pat { aspect-ratio: 1; min-width: 0; border-radius: calc(10px * var(--rk, 1)); border: 1px solid var(--border); background: var(--bg-input);
    font-size: 17px; line-height: 1; cursor: pointer; color: var(--fg-muted); display: grid; place-items: center; padding: 0;
    font-family: "Noto Color Emoji", "Apple Color Emoji", "Segoe UI Emoji", sans-serif; transition: transform .12s, border-color .12s; }
  .pat svg { width: 16px; height: 16px; }
  .pat:hover { transform: translateY(-1px); border-color: var(--border-strong); }
  .pat.on { border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent); background: var(--accent-soft); }
  .sheet.light .pat.on { border-color: var(--fg); box-shadow: 0 0 0 1px var(--fg); }
  .pat.own { font-family: inherit; font-weight: 600; }
  .pat-own-wrap { position: relative; display: grid; }
  .bn-cols { display: flex; align-items: center; gap: 10px; }
  .bn-arrow { color: var(--fg-dim); }
  .username { display: flex; align-items: center; gap: 6px; }
  .uid-chip { margin: 6px 0 12px; align-self: flex-start; padding: 2px 9px; border-radius: 999px; border: 1px solid var(--border); background: transparent;
    color: var(--fg-dim); font: 600 11.5px var(--font-mono, monospace); letter-spacing: .04em; cursor: pointer; }
  .uid-chip:hover { color: var(--fg); border-color: var(--border-strong); }
  .qr-btn { border: none; background: none; padding: 2px; color: var(--fg-dim); cursor: pointer; display: grid; place-items: center; }
  .qr-btn:hover { color: var(--accent); }
  .qr-btn svg { width: 16px; height: 16px; }
  .qr-pop { position: fixed; inset: 0; z-index: 140; display: grid; place-items: center; background: rgba(4, 6, 10, 0.6); backdrop-filter: blur(4px); }
  .qr-card { display: flex; flex-direction: column; align-items: center; gap: 8px; padding: 22px; border-radius: calc(22px * var(--rk, 1)); color: #fff;
    background: linear-gradient(160deg, var(--card), color-mix(in srgb, var(--card) 45%, #000)); box-shadow: 0 30px 70px -20px rgba(0,0,0,.7); }
  .qr-card b { font-size: 17px; }
  .qr-card small { opacity: .8; font-size: 12px; max-width: 220px; text-align: center; }

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
    border-radius: calc(16px * var(--rk, 1));
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

  /* A bigger banner (was 120 px). */
  .banner {
    height: 180px;
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
    padding: 6px 12px; border-radius: calc(10px * var(--rk, 1)); backdrop-filter: blur(6px); }
  .banner-hit:hover { background: rgba(0,0,0,.25); }
  .banner-hit:hover span, .banner-hit:focus-visible span { opacity: 1; transform: none; }
  .banner-tools { z-index: 3; position: absolute; right: 14px; bottom: 12px; display: flex; gap: 6px; align-items: center; }
  .bn-btn {
    display: inline-flex; align-items: center; gap: 6px;
    background: rgba(10, 12, 18, 0.5); color: #fff; border: 1px solid rgba(255, 255, 255, 0.18); border-radius: 100px;
    padding: 6px 12px; font-size: 12px; font-weight: 700; cursor: pointer; backdrop-filter: blur(10px);
  }
  .bn-btn svg { width: 14px; height: 14px; }
  .bn-btn.icon { padding: 6px; }
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
  .pv-row { display: flex; align-items: center; gap: 10px; padding: 9px 12px; background: var(--bg-input); border-radius: calc(10px * var(--rk, 1)); margin-bottom: 6px; }
  .pv-text { flex: 1; min-width: 0; }
  .pv-name { font-size: 13.5px; font-weight: 600; color: var(--fg); }
  .pv-eff { font-size: 11.5px; color: var(--accent); }
  .pv-eff.no { color: #ff8f8f; }
  .pv-seg { display: flex; background: var(--bg-shell); border: 1px solid var(--border); border-radius: calc(9px * var(--rk, 1)); padding: 2px; flex-shrink: 0; }
  .pv-seg button { border: none; background: none; color: var(--fg-dim); font: inherit; font-size: 11.5px; font-weight: 700; padding: 5px 8px; border-radius: calc(7px * var(--rk, 1)); cursor: pointer; }
  .pv-seg button.on { background: var(--bg-hover); color: var(--fg); }
  .pv-seg button.ok.on { background: color-mix(in srgb, var(--accent) 18%, transparent); color: var(--accent); }
  .pv-seg button.bad.on { background: rgba(255, 77, 109, 0.16); color: #ff8f8f; }
  .pv-badge { background: var(--accent); color: #06070C; border-radius: 100px; padding: 0 6px; font-size: 11px; margin-left: 4px; }
  .pub-note { font-size: 11.5px; color: var(--fg-dim); margin: 4px 0 10px; }
  .act-card {
    display: flex; align-items: center; gap: 12px;
    margin-top: 12px; padding: 10px 12px;
    background: var(--bg-input); border: 1px solid var(--border); border-radius: calc(12px * var(--rk, 1));
  }
  .act-ic { width: 40px; height: 40px; border-radius: calc(10px * var(--rk, 1)); display: grid; place-items: center; font-size: 20px; background: var(--accent-soft); flex-shrink: 0; }
  .act-line { font-size: 12px; color: var(--fg-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .close {
    position: absolute;
    z-index: 3;
    top: 10px; right: 10px;
    width: 28px; height: 28px;
    background: rgba(0, 0, 0, 0.35);
    border: none;
    border-radius: calc(8px * var(--rk, 1));
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
    margin-top: -52px;
  }

  .avatar-wrap { position: relative; width: 104px; margin-bottom: 14px; }
  .avatar {
    width: 104px; height: 104px;
    border-radius: var(--ava-r, 50%);
    display: grid; place-items: center;
    font-size: 40px;
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
  /* Offline: a hollow ring, like Discord. */
  .presence.offline, .presence.invisible { background: var(--bg-shell); box-shadow: inset 0 0 0 3px #80848E; }
  .presence.dnd { background: #ED4245; }
  .av-edit {
    position: absolute;
    right: -2px; bottom: 0;
    width: 30px; height: 30px; padding: 0;
    display: grid; place-items: center;
    background: var(--accent); color: #06070C;
    border: 3px solid var(--card-top); border-radius: 50%;
    font-size: 12px; font-weight: 700; cursor: pointer;
    box-shadow: 0 4px 10px -2px rgba(0, 0, 0, 0.4);
  }
  .av-edit svg { width: 14px; height: 14px; }
  .sheet.ed .presence { display: none; }
  .av-edit:hover { filter: brightness(1.08); }
  .av-remove {
    position: absolute;
    right: -2px; top: -2px;
    width: 22px; height: 22px;
    background: var(--danger); color: #fff;
    border: 2px solid var(--card-base); border-radius: 50%;
    font-size: 13px; line-height: 1; cursor: pointer;
    opacity: 0; transition: opacity .15s;
  }
  .avatar-wrap:hover .av-remove, .av-remove:focus-visible { opacity: 1; }

  .bot-tag {
    font-size: 9.5px; font-weight: 800; letter-spacing: 0.06em;
    background: var(--accent); color: #06070C;
    padding: 2px 6px; border-radius: calc(5px * var(--rk, 1));
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
    border-radius: calc(12px * var(--rk, 1)); text-decoration: none;
  }
  .np-card:hover { border-color: var(--accent); }
  .np-ic {
    width: 34px; height: 34px; flex-shrink: 0; border-radius: calc(9px * var(--rk, 1));
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
  /* The emoji sits right after the name (its own 4px margin + the gap was a hole). */
  .name :global(.es) { margin-left: -2px; }
  .badge :global(svg) { width: 16px; height: 16px; color: var(--accent); filter: drop-shadow(0 0 4px var(--accent-glow)); }
  .username {
    font-family: var(--font-mono);
    font-size: 12.5px;
    color: var(--fg-dim);
    margin: 2px 0 16px;
  }

  .section {
    background: var(--bg-input);
    border-radius: calc(10px * var(--rk, 1));
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
    border-radius: calc(8px * var(--rk, 1));
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
    display: flex; justify-content: flex-end; flex-wrap: wrap; gap: 8px;
    margin-top: 16px;
  }
  .buttons > button { white-space: nowrap; }
  .btn-ghost, .btn-primary {
    padding: 8px 16px;
    border-radius: calc(10px * var(--rk, 1));
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
