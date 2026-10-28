<script lang="ts">
  import { emojiStatusOf } from "../people";
  import EmojiStatus from "./EmojiStatus.svelte";
  import { onBack } from "../back";
  import AnimatedMedia from "./AnimatedMedia.svelte";
  import { hasMarkup, plainText } from "../markup";
  /** Chat-list line without formatting markers (spoilers stay hidden). */
  function previewText(p: string): string {
    if (p && unsupportedKind(p)) return tr("Unsupported message — update the app");
    // "📞 Missed call" etc. come from the backend in English.
    const call = p?.match(/^📞 ((?:Outgoing|Incoming|Missed|Declined|Cancelled) call|Call)$/);
    if (call) return "📞 " + tr(call[1]);
    // [ce:pack/😏_001.webp] → 😏 (the backend does it too; old previews don't)
    const q = p ? p.replace(/\[ce:[^\]\s]*?\/([^\]\s/_]+)_[^\]\s]*\]/g, "$1").replace(/\[ce:[^\]]*$/, "…") : p;
    return q && hasMarkup(q) ? plainText(q).replace(/\s*\n\s*/g, " ") : q;
  }
  import { chatStatus, presenceNow } from "../presence";
  import { unsupportedKind } from "../compat";
  import { t, tr } from "../i18n";
  import { fly, scale as scaleIn } from "svelte/transition";
  import { flip } from "svelte/animate";
  import { cubicOut, backOut } from "svelte/easing";
  import { avatars, peerCards, clans } from "../people";
  import ClanTag from "./ClanTag.svelte";
  import { keepOnScreen } from "../actions";
  import { archived, mutedChats, setArchived, setMuted, pinnedChats, togglePin, movePin, MAX_PINS, markedUnread, setMarkedUnread } from "../chatPrefs";
  import { folders as allFolders, isInFolder, toggleChatInFolder } from "../folders";
  import { parseJoinLink } from "../groupRules";
  import { groupMap, requestJoinGroup } from "../groups";
  import { roomsOf } from "../groupRules";
  import { rolesOn, hasPerm } from "../roles";
  import { activeGroupCalls } from "../groupCall";
  import { openRoomRequest } from "../roomNav";
  import { openPopout } from "../popouts";
  import type { Room } from "../types";
  import { peerPresence, activityVerb } from "../presence";
  import { icons } from "../icons";
  import { info, search, visibleChats, activeId, selectChat, startChat, chats, connect, backToList } from "../stores";
  import { askConfirm } from "./Confirm.svelte";
  import { showToast } from "./Toast.svelte";
  import { openNewChatDialog } from "../newChat";
  import { openNewChat } from "./NewChatSheet.svelte";
  import { openNewGroup } from "./NewGroupWizard.svelte";

  let plusMenuOpen = $state(false);
  import { contacts, saveContact } from "../contacts";
  import { api } from "../tauri";
  import { blocked, setBlocked } from "../ghost";
  import { chatLocks, unlocked, lockNowChat } from "../chatLocks";
  import { askSetLock, askRemoveLock } from "../chatLockUi";
  import { askInput, closeInput } from "./InputPrompt.svelte";
  import type { DirProfile, ChatMessage } from "../types";
  import { assetUrl } from "../tauri";
  import { folders, activeFolder } from "../folders";
  import { chatGroups, openGroupId, createGroup, addToGroup, removeFromGroup, type ChatGroup } from "../chatGroups";
  import type { ChatSummary } from "../types";
  import { openFolderManager } from "./FolderManager.svelte";
  import { openProfile } from "./ProfileModal.svelte";
  import { profile } from "../profile";
  import { ping } from "../ping";
  import ServerPicker from "./ServerPicker.svelte";
  import { accounts, loadAccounts, switchAccount } from "../accounts";

  // ---- Phones: your avatar in the title row. Tap — your profile; hold —
  // the accounts on this device (switch, or add one more). ----
  const meLetter = $derived(((($profile.display_name && $profile.display_name !== "You") ? $profile.display_name : $info?.username) || "?").charAt(0).toUpperCase());
  let accountsOpen = $state(false);
  let switching = $state(false);
  let meTimer: ReturnType<typeof setTimeout> | null = null;
  let meHeld = false;
  function meDown(e: PointerEvent) {
    meHeld = false;
    if (e.button !== 0) return;
    if (meTimer) clearTimeout(meTimer);
    meTimer = setTimeout(() => { meTimer = null; meHeld = true; openAccounts(); }, 420);
  }
  function meCancel() { if (meTimer) { clearTimeout(meTimer); meTimer = null; } }
  function meClick() { meCancel(); if (meHeld) { meHeld = false; return; } openProfile(); }
  function openAccounts() {
    try { navigator.vibrate?.(12); } catch { /* no vibration */ }
    accountsOpen = true;
    void loadAccounts();
  }
  async function pickAccount(slot: string) {
    if (switching) return;
    switching = true;
    try { await switchAccount(slot); } catch (e) { switching = false; showToast(String(e)); }
  }
  async function forgetAccount(slot: string) {
    try { await api.accountForget(slot); await loadAccounts(); } catch (e) { showToast(String(e)); }
  }
  // Back closes the sheets and the preview (lib/back.ts).
  $effect(() => { if (accountsOpen) return onBack(() => (accountsOpen = false)); });
  $effect(() => { if (serversOpen) return onBack(() => (serversOpen = false)); });
  $effect(() => { if (peek) return onBack(() => (peek = null)); });
  // ---- The servers sheet: which relay we're on, the others, reconnect ----
  let serversOpen = $state(false);
  let srvUrl = $state("");
  let srvBusy = $state(false);
  let srvErr = $state<string | null>(null);
  function openServers() {
    srvUrl = $info?.server_url ?? "";
    srvErr = null;
    serversOpen = true;
  }
  async function reconnectTo() {
    const user = $info?.username;
    if (!user || !srvUrl.trim()) return;
    srvBusy = true; srvErr = null;
    const err = await connect(srvUrl.trim(), user);
    srvBusy = false;
    if (err) srvErr = err; else serversOpen = false;
  }
  const hostOf = (u: string | null) => { try { return u ? new URL(u).host : ""; } catch { return u ?? ""; } };

  // ---- Archive (Telegram): archived chats live behind one row ----
  let showArchive = $state(false);
  let archivedChats = $derived($visibleChats.filter((c) => $archived.has(c.id)));
  let archivedUnread = $derived(archivedChats.filter((c) => c.unread > 0 && !$mutedChats.has(c.id)).length);
  $effect(() => { if (showArchive && archivedChats.length === 0) showArchive = false; });

  // Chats filtered by archive + the active folder ("all" shows everything).
  // A search looks through archived chats too.
  let shownChats = $derived.by(() => {
    const af = $activeFolder;
    let list = $visibleChats;
    if (!$search.trim()) list = list.filter((c) => (showArchive ? $archived.has(c.id) : !$archived.has(c.id)));
    if (af !== "all" && !showArchive) {
      const f = $folders.find((x) => x.id === af);
      if (f) { const set = new Set(f.chatIds); list = list.filter((c) => set.has(c.id)); }
    }
    // Pinned chats on top, in pin order (not while searching).
    if ($search.trim() || !$pinnedChats.length) return list;
    const rank = new Map($pinnedChats.map((id, i) => [id, i]));
    const pinned = list.filter((c) => rank.has(c.id)).sort((a, b) => rank.get(a.id)! - rank.get(b.id)!);
    return [...pinned, ...list.filter((c) => !rank.has(c.id))];
  });
  // ---- Chat groups: a folded group is one row, at its newest chat's place ----
  interface Entry { key: string; c: ChatSummary; g?: ChatGroup; members?: ChatSummary[] }
  let groupByChat = $derived.by(() => {
    const m = new Map<string, ChatGroup>();
    for (const g of $chatGroups) for (const id of g.chatIds) m.set(id, g);
    return m;
  });
  let entries = $derived.by((): Entry[] => {
    if ($search.trim() || showArchive) return shownChats.map((c) => ({ key: c.id, c }));
    const out: Entry[] = [];
    const placed = new Set<string>();
    for (const c of shownChats) {
      const g = groupByChat.get(c.id);
      if (g?.collapsed) {
        if (placed.has(g.id)) continue;
        placed.add(g.id);
        const set = new Set(g.chatIds);
        out.push({ key: g.id, c, g, members: shownChats.filter((x) => set.has(x.id)) });
      } else {
        out.push({ key: c.id, c });
      }
    }
    return out;
  });
  const groupUnread = (e: Entry) => (e.members ?? []).reduce((n, m) => n + (m.id === $activeId ? 0 : m.unread), 0);
  /** "CeZeOs news" inside group "CeZeOs" → "news": the group name is already on the row. */
  function memberLabel(group: string, chat: string): string {
    const rest = chat.toLowerCase().startsWith(group.toLowerCase()) ? chat.slice(group.length).replace(/^[\s·:|\-–—]+/, "") : "";
    return rest || chat;
  }
  const groupMuted = (e: Entry) => (e.members ?? []).every((m) => m.channel || $mutedChats.has(m.id));

  /** Chat menu → "Add to a group…": pick one, or name a new one. */
  let groupPick = $state<{ x: number; y: number; id: string } | null>(null);
  function newGroupFor(id: string) {
    groupPick = null;
    askInput({
      title: tr("New chat group"),
      description: tr("Chats of a group can be shown as one row of the list."),
      fields: [{ label: tr("Name") }],
      submitLabel: tr("Create"),
      onSubmit: async ([n]) => {
        if (!(n ?? "").trim()) return tr("Enter a name");
        const gid = createGroup(n ?? "", [id]);
        closeInput();
        openGroupId.set(gid);
        return null;
      },
    });
  }

  function pinChat(id: string) {
    const was = $pinnedChats.includes(id);
    if (!togglePin(id)) showToast(tr("You can pin up to {n} chats", { n: MAX_PINS }));
    else showToast(was ? tr("Unpinned") : tr("Pinned to the top"));
    ctx = null;
  }

  // ---- Reorder pinned chats by dragging ----
  let dragPin = $state<{ id: string; y0: number; active: boolean; over: number } | null>(null);
  let suppressClick = false;
  function pinDown(e: PointerEvent, id: string) {
    if (e.button !== 0 || !$pinnedChats.includes(id) || $search.trim()) return;
    dragPin = { id, y0: e.clientY, active: false, over: $pinnedChats.indexOf(id) };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function pinMove(e: PointerEvent) {
    if (!dragPin) return;
    if (!dragPin.active && Math.abs(e.clientY - dragPin.y0) < 6) return;
    dragPin.active = true;
    const rows = [...document.querySelectorAll<HTMLElement>(".chat-item.pinned")];
    let over = rows.length - 1;
    for (let i = 0; i < rows.length; i++) { const r = rows[i].getBoundingClientRect(); if (e.clientY < r.top + r.height / 2) { over = i; break; } }
    dragPin.over = over;
  }
  function pinUp() {
    if (dragPin?.active) { movePin(dragPin.id, dragPin.over); suppressClick = true; setTimeout(() => (suppressClick = false), 0); }
    dragPin = null;
  }

  // ---- A server (supergroup) is open: the list shrinks to avatars, like
  // Discord's server column, and grows back for any other chat. ----
  let compactPinned = $state(false); // the user opened it wide while in a server
  const inServer = $derived(!!$activeId && $groupMap[$activeId]?.kind === "supergroup");
  // Phones show one panel at a time: the avatar column makes no sense there.
  let phone = $state(typeof window !== "undefined" && window.matchMedia("(max-width: 720px)").matches);
  $effect(() => {
    const mq = window.matchMedia("(max-width: 720px)");
    const on = () => (phone = mq.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  });
  const compact = $derived(inServer && !compactPinned && !phone);
  $effect(() => { if (!inServer) compactPinned = false; });
  // Opening the archive or switching folders means browsing the list:
  // show it wide, with avatars and names.
  $effect(() => { if (showArchive && inServer) compactPinned = true; });
  let lastFolder = $activeFolder;
  $effect(() => { const f = $activeFolder; if (f !== lastFolder) { lastFolder = f; if (inServer) compactPinned = true; } });
  $effect(() => { document.documentElement.classList.toggle("list-compact", compact); });

  // ---- Resizable list (drag the right edge) ----
  const LW_KEY = "aqua.listWidth";
  function applyListWidth(w: number) { document.documentElement.style.setProperty("--list-w", `${w}px`); }
  try { const w = +(localStorage.getItem(LW_KEY) ?? 0); if (w) applyListWidth(w); } catch { /* ignore */ }
  let resizing = $state(false);
  function startResize(e: PointerEvent) {
    const handle = e.currentTarget as HTMLElement;
    const aside = handle.parentElement!;
    handle.setPointerCapture(e.pointerId);
    resizing = true;
    const left = aside.getBoundingClientRect().left;
    const move = (ev: PointerEvent) => applyListWidth(Math.round(Math.min(600, Math.max(240, ev.clientX - left))));
    const up = () => {
      resizing = false;
      handle.removeEventListener("pointermove", move); handle.removeEventListener("pointerup", up);
      try { localStorage.setItem(LW_KEY, String(Math.round(aside.getBoundingClientRect().width))); } catch { /* ignore */ }
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up);
  }
  function resetWidth() { document.documentElement.style.removeProperty("--list-w"); try { localStorage.removeItem(LW_KEY); } catch { /* ignore */ } }

  // ---- Per-chat passwords (only for me): lib/chatLockUi.ts ----

  // ---- Ctrl+click (or Ctrl+right-click): the chat in its own window ----
  const DESKTOP = typeof navigator !== "undefined" && !/android|iphone|ipad/i.test(navigator.userAgent);
  function popOut(id: string, name: string) {
    ctx = null;
    void openPopout(id, name).catch((e) => showToast(String(e)));
  }

  // Lists move (a chat jumps to the top) and appear with a short glide.
  const calm = typeof window !== "undefined" && (matchMedia("(prefers-reduced-motion: reduce)").matches || document.documentElement.classList.contains("soft-render") || document.documentElement.classList.contains("wk-lowfx"));
  const motionMs = (ms: number) => (calm ? 0 : ms);

  // ---- Hold an avatar: peek into the chat without opening it (Telegram) ----
  let peek = $state<{ id: string; msgs: ChatMessage[] } | null>(null);
  let peekTimer: ReturnType<typeof setTimeout> | null = null;
  let peekStart = { x: 0, y: 0 };
  let peekFired = false;
  function peekDown(e: PointerEvent, id: string) {
    if (e.button !== 0) return;
    peekFired = false;
    peekStart = { x: e.clientX, y: e.clientY };
    if (peekTimer) clearTimeout(peekTimer);
    peekTimer = setTimeout(() => { peekTimer = null; void openPeek(id); }, 380);
  }
  function peekMove(e: PointerEvent) {
    if (peekTimer && Math.hypot(e.clientX - peekStart.x, e.clientY - peekStart.y) > 8) { clearTimeout(peekTimer); peekTimer = null; }
  }
  function peekCancel() {
    if (peekTimer) { clearTimeout(peekTimer); peekTimer = null; }
  }
  // ---- A server (supergroup with rooms): holding its icon lists every
  // room you can open right now — text rooms ("chats") first, then voice.
  let roomsPop = $state<{ gid: string; x: number; y: number } | null>(null);
  const popRooms = $derived.by(() => {
    if (!roomsPop) return { text: [] as Room[], voice: [] as Room[] };
    const g = $groupMap[roomsPop.gid];
    const me = $info?.username ?? "";
    const rs = roomsOf(g).filter((r) => !rolesOn(g) || hasPerm(g, me, r.kind === "voice" ? "connect" : "view_room", r));
    return { text: rs.filter((r) => r.kind === "text"), voice: rs.filter((r) => r.kind === "voice") };
  });
  function goRoom(r: Room) {
    const gid = roomsPop?.gid;
    roomsPop = null;
    if (!gid) return;
    openRoomRequest.set({ gid, room: r.id });
    void selectChat(gid);
  }
  $effect(() => { if (roomsPop) return onBack(() => (roomsPop = null)); });

  async function openPeek(id: string) {
    if ($chatLocks[id] && !$unlocked[id]) return;            // a locked chat stays closed
    if (roomsOf($groupMap[id]).length) {
      peekFired = true;
      suppressClick = true; setTimeout(() => (suppressClick = false), 600);
      try { navigator.vibrate?.(12); } catch { /* no vibration */ }
      roomsPop = { gid: id, x: peekStart.x, y: peekStart.y };
      return;
    }
    peekFired = true;
    dragPin = null;
    suppressClick = true; setTimeout(() => (suppressClick = false), 600);
    try { navigator.vibrate?.(12); } catch { /* no vibration */ }
    let msgs: ChatMessage[] = [];
    try { msgs = await api.getMessages(id); } catch { /* show the header anyway */ }
    peekAt = Date.now();
    peek = { id, msgs: msgs.filter((m) => m.kind.type !== "date_separator").slice(-30) };
  }
  // The veil appears under the finger that is still holding the avatar: the
  // long press's own contextmenu (and a stray click on release) land on it
  // and must not close the peek they opened.
  let peekAt = 0;
  const peekSettled = () => Date.now() - peekAt > 650;
  function closePeek() { peek = null; }
  function dismissPeek() { if (peekSettled()) peek = null; }
  // Opening from the peek: the card grows to the whole window first
  // (Telegram), then the chat is there.
  let peekGrow = $state(false);
  function openFromPeek() {
    if (!peekSettled()) return;
    const id = peek?.id;
    if (!id) return;
    if (calm) { peek = null; void selectChat(id); return; }
    peekGrow = true;
    void selectChat(id);
    setTimeout(() => { peek = null; peekGrow = false; }, 240);
  }
  let peekFolders = $state(false);
  // Opening a chat takes its "unread" mark off.
  $effect(() => { const id = $activeId; if (id && $markedUnread.has(id)) setMarkedUnread(id, false); });
  let peekScroll = $state<HTMLElement | null>(null);
  $effect(() => { if (peek && peekScroll) peekScroll.scrollTop = peekScroll.scrollHeight; });
  function isMine(m: ChatMessage) { return m.from === "me" || (!!$info?.username && m.from === $info.username); }
  function peekImage(m: ChatMessage): string | null {
    if (m.kind.type !== "file") return null;
    const c = m.kind.content;
    const pic = c.category === "image" || c.category === "sticker" || c.category === "gif" || /^image\//.test(c.mime ?? "");
    return pic && c.saved_at ? assetUrl(c.saved_at) : null;
  }
  function peekText(m: ChatMessage): string {
    const k = m.kind;
    switch (k.type) {
      case "text": return previewText(k.content.text);
      case "reply": return previewText(k.content.text);
      case "code": return k.content.code.split("\n").slice(0, 4).join("\n");
      case "system": return k.content.text.startsWith("[call:v1]") ? "📞 " + tr("Call") : k.content.text;
      case "article": return "📰 " + k.content.title;
      case "poll": return "📊 " + k.content.question;
      case "file": {
        const c = k.content;
        if (c.category === "voice") return "🎤 " + tr("Voice message");
        if (c.category === "sticker") return "🖼 " + tr("Sticker");
        if (c.category === "gif") return "GIF";
        if (c.category === "video") return "🎬 " + c.name;
        return "📎 " + c.name;
      }
      default: return "";
    }
  }
  function peekStatus(id: string): string {
    const c = $chats.find((x) => x.id === id);
    if (!c) return "";
    if (id.startsWith("grp_")) { const g = $groupMap[id]; return g ? `${g.members.length} ${tr("members")}` : ""; }
    const st = chatStatus(c, $peerPresence, $presenceNow);
    return st ? tr(st === "online" ? "Online" : st === "idle" ? "Idle" : st === "dnd" ? "Do not disturb" : "Offline") : "";
  }
  function markRead(id: string) {
    chats.update((cs) => cs.map((c) => (c.id === id ? { ...c, unread: 0 } : c)));
    void api.markChatRead(id).catch(() => {});
  }
  /** "Mark all as read": every chat of a folder (or all of them). */
  function markAllRead(ids?: string[]) {
    const set = ids ? new Set(ids) : null;
    const unread = $chats.filter((c) => c.unread > 0 && (!set || set.has(c.id)));
    for (const c of unread) markRead(c.id);
    if (unread.length) showToast(tr("Marked as read: {n}", { n: unread.length }));
  }
  // A folder tab held / right-clicked: its menu.
  let folderMenu = $state<{ x: number; y: number; id: string } | null>(null);
  function openFolderMenu(e: MouseEvent, id: string) {
    e.preventDefault();
    folderMenu = { x: Math.min(e.clientX, window.innerWidth - 220), y: e.clientY + 8, id };
  }

  // ---- Right-click a chat: archive / mute ----
  let ctx = $state<{ x: number; y: number; id: string } | null>(null);
  function openCtx(e: MouseEvent, id: string) {
    e.preventDefault();
    ctx = { x: Math.min(e.clientX, window.innerWidth - 200), y: Math.min(e.clientY, window.innerHeight - 140), id };
  }
  function archiveChat(id: string) {
    const on = !$archived.has(id);
    setArchived(id, on);
    showToast(on ? "Chat archived" : "Chat moved back to the list");
    ctx = null;
  }

  /** Telegram's "Delete chat" (this device): the history goes; a new
   *  message from them starts it again. */
  async function deleteChat(id: string) {
    ctx = null;
    peek = null;
    const c = $chats.find((x) => x.id === id);
    const saved = id === "__saved__";
    const ok = await askConfirm({
      title: tr(saved ? "Clear Saved Messages?" : "Delete the chat with {name}?", { name: c?.name ?? id }),
      text: tr("The messages are removed from this device. This can't be undone."),
      ok: tr("Delete"),
      danger: true,
    });
    if (!ok) return;
    try {
      await api.deleteChat(id);
      if ($activeId === id) { backToList(); activeId.set(""); }
      setArchived(id, false);
      chats.set(await api.listChats());
    } catch (e) { showToast(String(e)); }
  }

  // ---- Invite links pasted into the search box ----
  let joinLink = $derived(parseJoinLink($search ?? ""));
  let joinIsMember = $derived(!!joinLink && !!$groupMap[joinLink.gid]?.members.some((m) => m.username === $info?.username));
  let joinSent = $state("");
  async function requestJoin() {
    const j = joinLink;
    if (!j) return;
    try {
      await requestJoinGroup(j.gid, j.admin, j.title, "");
      joinSent = j.gid;
      showToast(`Request sent to @${j.admin}`);
    } catch (e) {
      showToast(`Could not send request: ${e}`);
    }
  }

  // ---- Keyboard: ↑/↓ through the list from the search box, Enter opens ----
  let kbIndex = $state(-1);
  $effect(() => { void $search; kbIndex = $search ? 0 : -1; });
  function onSearchKey(e: KeyboardEvent) {
    const n = entries.length;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      if (!n) return;
      e.preventDefault();
      kbIndex = e.key === "ArrowDown" ? (kbIndex + 1) % n : (kbIndex - 1 + n) % n;
      document.querySelectorAll(".chat-item")[kbIndex]?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter") {
      const en = entries[kbIndex >= 0 ? kbIndex : 0];
      if (!en) return;
      e.preventDefault();
      if (en.g) { openGroupId.set(en.g.id); return; }
      const c = en.c;
      void selectChat(c.id);
      search.set("");
      kbIndex = -1;
      (e.target as HTMLInputElement).blur();
      queueMicrotask(() => window.dispatchEvent(new CustomEvent("aqua:focus-composer")));
    } else if (e.key === "Escape") {
      if ($search) { search.set(""); } else (e.target as HTMLInputElement).blur();
    }
  }

  async function copyFingerprint() {
    const fp = $info?.session_fingerprint ?? "";
    try {
      await navigator.clipboard.writeText(fp);
      showToast(tr("Session fingerprint copied"));
    } catch {
      showToast(tr("Copy blocked by browser"));
    }
  }

  // Decide whether the current search term "looks like" a username we
  // could reasonably try to start a chat with: non-empty, no spaces,
  // not already the name of a visible chat.
  function computeStartHint(term: string, chats: typeof $visibleChats): boolean {
    const t = term.trim().replace(/^@+/, "");
    if (t.length === 0) return false;
    if (t.includes("://")) return false;
    if (t.includes(" ")) return false;
    const lower = t.toLowerCase();
    return !chats.some(
      (c) => c.name.toLowerCase() === lower || c.id.toLowerCase() === lower,
    );
  }
  // "@ivan" works as well as "ivan".
  let searchTerm   = $derived(($search ?? "").trim().replace(/^@+/, ""));
  let showStartHint = $derived(computeStartHint($search ?? "", $visibleChats));

  // Live server-directory search. Debounced; results shown under the
  // "Start chat with" row so you can find people you've never chatted
  // with, by username OR display name.
  let dirResults = $state<DirProfile[]>([]);
  let dirSearching = $state(false);
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    const q = $search.trim().replace(/^@+/, "");
    if (debounceTimer) clearTimeout(debounceTimer);
    if (q.length < 2 || !$info?.connected) { dirResults = []; return; }
    debounceTimer = setTimeout(async () => {
      dirSearching = true;
      try {
        const r = await api.searchDirectory(q);
        // Hide people we already have a chat with (shown above).
        const existing = new Set($visibleChats.map((c) => c.id));
        dirResults = r.filter((p) => !existing.has(p.username));
      } catch {
        dirResults = [];
      } finally {
        dirSearching = false;
      }
    }, 280);
  });

  async function startWithDir(p: DirProfile) {
    if (p.display_name || p.phone) {
      saveContact(p.username, { full_name: p.display_name, phone: p.phone });
    }
    if (p.is_bot) {
      // Bots use a plaintext channel (no X3DH).
      try {
        await api.startBotChat(p.username, "/start");
        const { chats, selectChat, activeTab } = await import("../stores");
        chats.set(await api.listChats());
        activeTab.set("chats");
        await selectChat(p.username);
        search.set("");
      } catch (e) {
        showToast(`Could not start bot: ${e}`);
      }
      return;
    }
    // Their card and the first message — not an instant "Привет!".
    openNewChat(p);
    search.set("");
  }
</script>

<aside class="list" class:resizing class:compact class:has-active={!!$activeId}>
  {#if compact}
    <button class="compact-expand" onclick={() => (compactPinned = true)} title={$t("Show the chat list")} aria-label={$t("Show the chat list")}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M4 6h16M4 12h16M4 18h16"/></svg>
    </button>
  {/if}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="resize-handle" onpointerdown={startResize} ondblclick={resetWidth} title={$t("Drag to resize · double-click to reset")}></div>
  <!-- Phones: a title row like Telegram's ("Aquamarine" + menu). -->
  <div class="m-title">
    <button class="m-me" onpointerdown={meDown} onpointerup={meCancel} onpointercancel={meCancel} onpointerleave={meCancel}
      onclick={meClick} oncontextmenu={(e) => e.preventDefault()} aria-label={$t("Profile")} title={$t("Tap — profile, hold — accounts")}>
      {#if $profile.avatar_url}<img src={$profile.avatar_url} alt="" draggable="false" />{:else}{meLetter}{/if}
    </button>
    <span class="m-name">Aquamarine</span>
    <!-- The connection: ping when connected, "connecting" otherwise; tap —
         the servers sheet (switch server, see which ones answer). -->
    {#if $info}
      <button class="m-net" class:off={!$info.connected} class:slow={$info.connected && ($ping ?? 0) >= 150} class:bad={$info.connected && ($ping ?? 0) >= 400}
        onclick={openServers} title={$t("Server")} aria-label={$t("Server")}>
        {#if !$info.connected}<span class="m-spin"></span>{$t("Connecting…")}
        {:else if $ping != null}{$ping} {$t("ms")}
        {:else}<span class="m-dot"></span>{$t("online")}{/if}
      </button>
    {/if}
    <button class="m-menu" onclick={() => (plusMenuOpen = !plusMenuOpen)} aria-label={$t("New")}>
      <svg viewBox="0 0 24 24" fill="currentColor"><circle cx="12" cy="5" r="2"/><circle cx="12" cy="12" r="2"/><circle cx="12" cy="19" r="2"/></svg>
    </button>
  </div>
  <div class="list-header">
    {#if inServer && !compact}
      <button class="collapse-btn" onclick={() => (compactPinned = false)} title={$t("Collapse to avatars")} aria-label={$t("Collapse to avatars")}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
      </button>
    {/if}
    <label class="search">
      {@html icons.search}
      <input
        id="search"
        placeholder={$t("Search by name, phone or @username")}
        autocomplete="off"
        bind:value={$search}
        onkeydown={onSearchKey}
        onblur={() => (kbIndex = -1)}
      />
      <span class="search-kbd">{$t("Ctrl K")}</span>
    </label>
    <div class="plus-wrap">
      <button
        class="plus-btn"
        class:nudge={$visibleChats.every((c) => !c.live)}
        onclick={() => (plusMenuOpen = !plusMenuOpen)}
        title={$t("New chat, group or channel")}
        aria-label={$t("New")}
      >
        <svg class="plus-glyph" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>
      </button>
      {#if plusMenuOpen}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="plus-pop" role="menu" tabindex="-1">
          <button onclick={() => { plusMenuOpen = false; openNewChatDialog(); }}>{$t("New chat")}</button>
          <button onclick={() => { plusMenuOpen = false; openNewGroup("group"); }}>{$t("New group")}</button>
          <button onclick={() => { plusMenuOpen = false; openNewGroup("channel"); }}>{$t("New channel")}</button>
          {#if $chats.some((c) => c.unread > 0)}<button onclick={() => { plusMenuOpen = false; markAllRead(); }}>{$t("Mark all as read")}</button>{/if}
        </div>
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="plus-scrim" onclick={() => (plusMenuOpen = false)}></div>
      {/if}
    </div>
  </div>

  {#if $folders.length > 0}
    <div class="folder-strip">
      <button class="folder-tab" class:active={$activeFolder === "all"} onclick={() => activeFolder.set("all")} oncontextmenu={(e) => openFolderMenu(e, "all")}>{$t("All")}</button>
      {#each $folders as f (f.id)}
        <button class="folder-tab" class:active={$activeFolder === f.id} onclick={() => activeFolder.set(f.id)} oncontextmenu={(e) => openFolderMenu(e, f.id)}>
          {f.name}{#if f.chatIds.length > 0}<span class="ft-count">{f.chatIds.length}</span>{/if}
        </button>
      {/each}
      <button class="folder-edit" onclick={openFolderManager} title={$t("Manage folders")}>+</button>
    </div>
  {:else}
    <div class="folder-strip empty-strip">
      <button class="folder-tab active" oncontextmenu={(e) => openFolderMenu(e, "all")}>{$t("All")}</button>
      <button class="folder-edit" onclick={openFolderManager} title={$t("Create folders")}>{$t("+ Folders")}</button>
    </div>
  {/if}
  {#if folderMenu}
    {@const fm = folderMenu}
    <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
    <div class="ctx-scrim" onclick={() => (folderMenu = null)} oncontextmenu={(e) => { e.preventDefault(); folderMenu = null; }}></div>
    <div class="chat-ctx" use:keepOnScreen style:left="{fm.x}px" style:top="{fm.y}px" role="menu">
      <button onclick={() => { markAllRead(fm.id === "all" ? undefined : $folders.find((f) => f.id === fm.id)?.chatIds ?? []); folderMenu = null; }}>{$t("Mark all as read")}</button>
      <button onclick={() => { folderMenu = null; openFolderManager(); }}>{$t("Manage folders")}</button>
    </div>
  {/if}

  <div class="chats">
    {#if joinLink}
      <div class="join-card">
        <div class="jc-ava">{joinLink.title.charAt(0).toUpperCase()}</div>
        <div class="jc-body">
          <div class="jc-title">{joinLink.title}</div>
          <div class="jc-sub">Invite link · admin @{joinLink.admin}</div>
        </div>
        {#if joinIsMember}
          <button class="jc-btn" onclick={() => { const id = joinLink!.gid; search.set(""); void selectChat(id); }}>{$t("Open")}</button>
        {:else if joinSent === joinLink.gid}
          <span class="jc-done">{$t("Requested")}</span>
        {:else}
          <button class="jc-btn" onclick={requestJoin}>{$t("Request to join")}</button>
        {/if}
      </div>
    {/if}
    {#if !$search.trim() && showArchive}
      <button class="arch-back" onclick={() => (showArchive = false)}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
        {$t("Archived chats")}
      </button>
    {:else if !$search.trim() && archivedChats.length > 0 && $activeFolder === "all"}
      <button class="chat-item arch-row" onclick={() => (showArchive = true)} oncontextmenu={(e) => { e.preventDefault(); markAllRead(archivedChats.map((c) => c.id)); }}>
        <div class="arch-ic">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="5" rx="1.5"/><path d="M5 9v9a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V9"/><line x1="10" y1="13" x2="14" y2="13"/></svg>
        </div>
        <div class="chat-body">
          <div class="chat-row-1"><span class="chat-name">{$t("Archived chats")}</span></div>
          <div class="chat-row-2">
            <span class="chat-preview">{archivedChats.map((c) => c.name).slice(0, 4).join(", ")}</span>
            {#if archivedUnread > 0}<span class="chat-badge muted">{archivedUnread}</span>{/if}
          </div>
        </div>
      </button>
    {/if}
    {#if dirResults.length > 0}
      <div class="dir-head">{$t("Found on the server")}</div>
      {#each dirResults as p (p.username)}
        <button class="dir-row" onclick={() => startWithDir(p)} title={`Start a chat with @${p.username}`}>
          <div class="dir-avatar">{(p.display_name || p.username).charAt(0).toUpperCase()}</div>
          <div class="dir-body">
            <div class="dir-name">
              {p.display_name || p.username}
              {#if p.is_bot}<span class="bot-tag">BOT</span>{/if}
            </div>
            <div class="dir-sub">@{p.username}{#if p.about} · {p.about}{/if}</div>
          </div>
          <span class="dir-go">+</span>
        </button>
      {/each}
    {/if}
    {#if showStartHint && dirResults.length === 0 && !dirSearching}
      <button
        class="start-row"
        onclick={() => openNewChatDialog(searchTerm)}
        title={`Start a new chat with "${searchTerm}" on the server`}
      >
        <div class="start-icon">{@html icons.edit}</div>
        <div class="start-body">
          <div class="start-title">{$t("Start chat with")} <b>“{searchTerm}”</b></div>
          <div class="start-sub">{$t("No directory match — click to open a chat with that exact username")}</div>
        </div>
      </button>
    {/if}
    {#each entries as en, ci (en.key)}
      {@const c = en.c}
      {@const g = en.g}
      {@const st = g ? null : chatStatus(c, $peerPresence, $presenceNow)}
      {@const lk = !g && !!$chatLocks[c.id] && !$unlocked[c.id]}
      {@const inGroup = g ? undefined : groupByChat.get(c.id)}
      <button
        class="chat-item"
        class:grp={!!g}
        class:alt={ci % 2 === 1}
        animate:flip={{ duration: motionMs(260), easing: cubicOut }}
        in:fly|global={{ y: 10, duration: motionMs(300), delay: Math.min(ci, 14) * 22, easing: cubicOut }}
        title={compact ? (g ? g.name : c.name) : undefined}
        class:active={!g && c.id === $activeId}
        class:kb={ci === kbIndex}
        class:pinned={!g && $pinnedChats.includes(c.id)}
        class:dragging={!g && dragPin?.active && dragPin.id === c.id}
        class:drop-before={!g && dragPin?.active && dragPin.id !== c.id && $pinnedChats.indexOf(c.id) === dragPin.over}
        onpointerdown={(e) => { if (!g) { pinDown(e, c.id); if (e.pointerType === "touch") peekDown(e, c.id); } }}
        onpointermove={(e) => { peekMove(e); pinMove(e); }}
        onpointerup={() => { peekCancel(); pinUp(); }}
        onpointercancel={() => { peekCancel(); dragPin = null; }}
        onclick={(e) => { if (suppressClick) return; if (g) { openGroupId.set(g.id); return; } if ((e.ctrlKey || e.metaKey) && DESKTOP) { popOut(c.id, c.name); return; } void selectChat(c.id); }}
        oncontextmenu={(e) => { if (g) { e.preventDefault(); openGroupId.set(g.id); return; } if (peekFired || peek) { e.preventDefault(); return; } if ((e.ctrlKey || e.metaKey) && DESKTOP) { e.preventDefault(); popOut(c.id, c.name); return; } openCtx(e, c.id); }}
      >
        {#if g}
          {@const n = groupUnread(en)}
          <div class="gstack">
            {#if g.emoji}<span class="gemo">{g.emoji}</span>
            {:else}
              {#each (en.members ?? []).slice(0, 3) as m, k (m.id)}
                <span class="avatar gmini" style="--gradient: {m.gradient}; --k: {k}">{m.letter}{#if $avatars[m.id]}<img class="ava-img" src={$avatars[m.id]} alt="" loading="lazy" />{/if}</span>
              {/each}
            {/if}
            {#if compact && n > 0}<span class="cmp-badge" class:muted={groupMuted(en)}>{n > 99 ? "99+" : n}</span>{/if}
          </div>
          <div class="chat-body">
            <div class="chat-row-1">
              <span class="chat-name"><span class="cn-text">{g.name}</span><span class="g-count">{(en.members ?? []).length}</span></span>
              <span class="chat-time">{c.time}</span>
            </div>
            <div class="chat-row-2">
              <span class="chat-preview"><span class="me-prefix">{memberLabel(g.name, c.name)}:</span> {previewText(c.preview)}</span>
              {#if n > 0}
                {#key n}<span class="chat-badge" class:muted={groupMuted(en)} in:scaleIn={{ start: 0.4, duration: 260, easing: backOut }}>{n}</span>{/key}
              {/if}
            </div>
          </div>
        {:else}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="avatar sz-44" class:lk class:ava-grp={c.id.startsWith("grp_") && !c.channel} class:ava-chan={c.channel} style="--gradient: {c.gradient}" onpointerdown={(e) => { if (e.pointerType !== "touch") peekDown(e, c.id); }} title={compact ? undefined : $t("Hold to peek")}>
          {lk ? "" : c.letter}
          {#if $avatars[c.id] && !lk}<img class="ava-img" src={$avatars[c.id]} alt="" loading="lazy" />{/if}
          {#if lk}<span class="lk-ic">{@html icons.lock}</span>{/if}
          {#if st}<span class="st-dot {st}" title={$t(st === "online" ? "Online" : st === "idle" ? "Idle" : st === "dnd" ? "Do not disturb" : "Offline")}></span>{/if}
          {#if compact && c.unread > 0 && c.id !== $activeId}<span class="cmp-badge" class:muted={c.channel || $mutedChats.has(c.id)}>{c.unread > 99 ? "99+" : c.unread}</span>{/if}
        </div>
        <div class="chat-body">
          <div class="chat-row-1">
            <span class="chat-name">
              <span class="cn-text" class:lk-blur={lk}>{lk ? "•••••••" : c.name}</span>{#if !lk && !c.id.startsWith("grp_")}<EmojiStatus token={emojiStatusOf($peerCards, c.id)} size={16} />{/if}
              {#if inGroup}<!-- svelte-ignore a11y_no_static_element_interactions --><!-- svelte-ignore a11y_click_events_have_key_events --><span class="g-tag" onclick={(e) => { e.stopPropagation(); openGroupId.set(inGroup.id); }} title={$t("Chat group")}>{inGroup.emoji ?? "▣"} {inGroup.name}</span>{/if}
              {#if c.verified}<span class="verified-badge">{@html icons.verified}</span>{/if}
              {#if $clans[c.id]}<ClanTag tag={$clans[c.id]} />{/if}
              {#if $mutedChats.has(c.id)}<span class="muted-ic" title={$t("Muted")}><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 5 6 9H2v6h4l5 4V5z"/><line x1="23" y1="9" x2="17" y2="15"/><line x1="17" y1="9" x2="23" y2="15"/></svg></span>{/if}
            </span>
            <span class="chat-time">{c.time}</span>
          </div>
          <div class="chat-row-2">
            {#if lk}
              <span class="chat-preview lk-prev">{$t("Locked chat")}</span>
            {:else if c.preview_is_typing}
              <span class="chat-preview typing">{c.preview}</span>
            {:else if $peerPresence[c.id]?.activity?.[0]}
              {@const a = $peerPresence[c.id].activity![0]}
              <span class="chat-preview activity" title={`${activityVerb(a)} ${a.name}${a.details ? " — " + a.details : ""}`}>{activityVerb(a)} <b>{a.name}</b></span>
            {:else}
              <span class="chat-preview">
                {#if c.preview_by_me}<span class="me-prefix">{$t("you:")}</span>{/if}{previewText(c.preview)}
              </span>
            {/if}
            {#if c.unread > 0 && c.id !== $activeId}
              {#key c.unread}<span class="chat-badge" class:muted={c.channel || $mutedChats.has(c.id)} in:scaleIn={{ start: 0.4, duration: 260, easing: backOut }}>{c.unread}</span>{/key}
            {:else if $markedUnread.has(c.id) && c.id !== $activeId}
              <span class="chat-badge mark" title={$t("Marked as unread")}></span>
            {:else if $pinnedChats.includes(c.id)}
              <span class="pin-ic" title={$t("Pinned")}><svg viewBox="0 0 24 24" fill="currentColor"><path d="M16 3l5 5-3 1-4 4 1 5-2 2-4-4-5 5-1-1 5-5-4-4 2-2 5 1 4-4z"/></svg></span>
            {/if}
          </div>
        </div>
        {/if}
      </button>
    {/each}
  </div>

  {#if ctx}
    {@const cid = ctx.id}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="ctx-scrim" onclick={() => (ctx = null)} oncontextmenu={(e) => { e.preventDefault(); ctx = null; }}></div>
    <div class="chat-ctx" use:keepOnScreen style:left="{ctx.x}px" style:top="{ctx.y}px" role="menu">
      <button onclick={() => pinChat(cid)}>{$t($pinnedChats.includes(cid) ? "Unpin" : "Pin to top")}</button>
      {#if $pinnedChats.includes(cid) && $pinnedChats.length > 1}
        {@const pi = $pinnedChats.indexOf(cid)}
        {#if pi > 0}<button onclick={() => { movePin(cid, pi - 1); ctx = null; }}>{$t("Move up")}</button>{/if}
        {#if pi < $pinnedChats.length - 1}<button onclick={() => { movePin(cid, pi + 1); ctx = null; }}>{$t("Move down")}</button>{/if}
      {/if}
      {#if ($chats.find((x) => x.id === cid)?.unread ?? 0) > 0}<button onclick={() => { markRead(cid); ctx = null; }}>{$t("Mark as read")}</button>{/if}
      <button onclick={() => archiveChat(cid)}>{$t($archived.has(cid) ? "Unarchive" : "Archive")}</button>
      {#if groupByChat.get(cid)}
        {@const cg = groupByChat.get(cid)!}
        <button onclick={() => { ctx = null; openGroupId.set(cg.id); }}>{$t("Open group “{name}”", { name: cg.name })}</button>
        <button onclick={() => { removeFromGroup(cid); ctx = null; }}>{$t("Remove from the group")}</button>
      {:else}
        <button onclick={(e) => { const r = (e.currentTarget as HTMLElement).getBoundingClientRect(); groupPick = { x: r.right + 4, y: r.top, id: cid }; ctx = null; }}>{$t("Add to a group…")} ›</button>
      {/if}
      <button onclick={() => { setMuted(cid); ctx = null; }}>{$t($mutedChats.has(cid) ? "Unmute" : "Mute notifications")}</button>
      <button onclick={() => { void selectChat(cid); ctx = null; }}>{$t("Open")}</button>
      {#if $chatLocks[cid]}
        <button onclick={() => { const id = cid; ctx = null; askRemoveLock(id); }}>{$t("Remove the password")}</button>
        {#if $unlocked[cid]}<button onclick={() => { const id = cid; ctx = null; lockNowChat(id); }}>{$t("Lock now")}</button>{/if}
      {:else}
        <button onclick={() => { const id = cid; ctx = null; askSetLock(id); }}>{$t("Set a password…")}</button>
      {/if}
      {#if DESKTOP}<button onclick={() => popOut(cid, $chats.find((x) => x.id === cid)?.name ?? cid)}>{$t("Open in a new window")} <span class="ctx-kbd">Ctrl+click</span></button>{/if}
      {#if !cid.startsWith("grp_") && cid !== "__saved__" && !$chats.find((x) => x.id === cid)?.channel}
        <button class="danger" onclick={() => { setBlocked(cid, !$blocked.has(cid)); showToast(tr($blocked.has(cid) ? "Blocked — their messages won't reach you" : "Unblocked")); ctx = null; }}>{$t($blocked.has(cid) ? "Unblock" : "Block")}</button>
      {/if}
      {#if !cid.startsWith("grp_")}<button class="danger" onclick={() => void deleteChat(cid)}>{$t(cid === "__saved__" ? "Clear history" : "Delete chat")}</button>{/if}
    </div>
  {/if}

  {#if groupPick}
    {@const gp = groupPick}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="ctx-scrim" onclick={() => (groupPick = null)} oncontextmenu={(e) => { e.preventDefault(); groupPick = null; }}></div>
    <div class="chat-ctx" use:keepOnScreen style:left="{gp.x}px" style:top="{gp.y}px" role="menu">
      {#each $chatGroups as cg (cg.id)}
        <button onclick={() => { addToGroup(cg.id, gp.id); groupPick = null; showToast(tr("Added to “{name}”", { name: cg.name })); }}>{cg.emoji ?? "▣"} {cg.name} <span class="ctx-kbd">{cg.chatIds.length}</span></button>
      {/each}
      <button onclick={() => newGroupFor(gp.id)}>＋ {$t("New group…")}</button>
    </div>
  {/if}

  {#if roomsPop}
    {@const g = $groupMap[roomsPop.gid]}
    <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
    <div class="rp-veil" onclick={() => (roomsPop = null)} oncontextmenu={(e) => e.preventDefault()}></div>
    <div class="rp" role="menu" aria-label={$t("Rooms")}
      style:left="{Math.min(roomsPop.x + 8, innerWidth - 268)}px" style:top="{Math.max(8, Math.min(roomsPop.y - 20, innerHeight - 380))}px">
      <div class="rp-title">{g?.title ?? ""}</div>
      {#if popRooms.text.length}<div class="rp-sec">{$t("Chats")}</div>{/if}
      {#each popRooms.text as r (r.id)}
        <button class="rp-row" role="menuitem" onclick={() => goRoom(r)}>
          <span class="rp-ic" style:color={r.color}>{r.emoji ?? "#"}</span>
          <span class="rp-name">{r.name}{#if r.topic}<small>{r.topic}</small>{/if}</span>
          {#if r.readonly}<span class="rp-tag">{$t("read-only")}</span>{/if}
        </button>
      {/each}
      {#if popRooms.voice.length}<div class="rp-sec">{$t("Voice")}</div>{/if}
      {#each popRooms.voice as r (r.id)}
        {@const live = $activeGroupCalls[roomsPop.gid]}
        <button class="rp-row" role="menuitem" onclick={() => goRoom(r)}>
          <span class="rp-ic">{r.emoji ?? "🔊"}</span>
          <span class="rp-name">{r.name}{#if live?.users?.length}<small>{tr("{n} in the room", { n: live.users.length })}</small>{/if}</span>
        </button>
      {/each}
      {#if !popRooms.text.length && !popRooms.voice.length}<div class="rp-empty">{$t("No rooms you can open")}</div>{/if}
    </div>
  {/if}
  {#if peek}
    {@const pc = $chats.find((x) => x.id === peek!.id)}
    {@const pid = peek.id}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="peek-veil" onclick={dismissPeek} oncontextmenu={(e) => e.preventDefault()}>
      <div class="peek" class:grow={peekGrow} onclick={(e) => e.stopPropagation()}>
        <button class="peek-card" onclick={openFromPeek} title={$t("Open")}>
          <div class="peek-head">
            <div class="avatar sz-40" style="--gradient: {pc?.gradient ?? ''}">
              {pc?.letter ?? "?"}
              {#if $avatars[pid]}<img class="ava-img" src={$avatars[pid]} alt="" />{/if}
            </div>
            <div class="peek-who">
              <div class="peek-name">{pc?.name ?? pid}</div>
              <div class="peek-st">{peekStatus(pid)}</div>
            </div>
          </div>
          <div class="peek-msgs" bind:this={peekScroll}>
            {#if peek.msgs.length === 0}
              <div class="peek-empty">{$t("No messages yet")}</div>
            {/if}
            {#each peek.msgs as m (m.id)}
              {#if m.kind.type === "system"}
                <div class="peek-sys">{peekText(m)}</div>
              {:else}
                {@const img = peekImage(m)}
                <div class="peek-row" class:mine={isMine(m)}>
                  <div class="peek-bubble" class:pic={!!img}>
                    {#if img && /\.(tgs|webm)$/i.test(m.kind.type === "file" ? m.kind.content.name : "")}<span class="peek-anim"><AnimatedMedia src={img} mime={m.kind.type === "file" && /\.tgs$/i.test(m.kind.content.name) ? "application/x-tgsticker" : "video/webm"} play="auto" /></span>{:else if img}<img src={img} alt="" loading="lazy" />{:else}<span class="peek-txt" class:code={m.kind.type === "code"}>{peekText(m)}</span>{/if}
                    <span class="peek-time">{m.time}</span>
                  </div>
                </div>
              {/if}
            {/each}
          </div>
        </button>
        <div class="peek-menu" role="menu">
          {#if peekFolders}
            {#each $allFolders as f (f.id)}
              <button onclick={() => toggleChatInFolder(f.id, pid)}>{isInFolder(f.id, pid) ? "✓ " : ""}{f.name}</button>
            {/each}
            <button onclick={() => { peekFolders = false; closePeek(); openFolderManager(); }}>{$t("New folder…")}</button>
            <button onclick={() => (peekFolders = false)}>‹ {$t("Back")}</button>
          {:else}
          <button onclick={openFromPeek}>{$t("Open")}</button>
          <button onclick={() => { if ($allFolders.length) peekFolders = true; else { closePeek(); openFolderManager(); } }}>{$t("Add to folder")}</button>
          {#if (pc?.unread ?? 0) > 0 || $markedUnread.has(pid)}<button onclick={() => { markRead(pid); setMarkedUnread(pid, false); closePeek(); }}>{$t("Mark as read")}</button>
          {:else}<button onclick={() => { setMarkedUnread(pid, true); closePeek(); }}>{$t("Mark as unread")}</button>{/if}
          <button onclick={() => { pinChat(pid); closePeek(); }}>{$t($pinnedChats.includes(pid) ? "Unpin" : "Pin to top")}</button>
          <button onclick={() => { setMuted(pid); closePeek(); }}>{$t($mutedChats.has(pid) ? "Unmute" : "Mute notifications")}</button>
          <button onclick={() => { archiveChat(pid); closePeek(); }}>{$t($archived.has(pid) ? "Unarchive" : "Archive")}</button>
          {#if !pid.startsWith("grp_")}<button class="danger" onclick={() => void deleteChat(pid)}>{$t(pid === "__saved__" ? "Clear history" : "Delete chat")}</button>{/if}
          {/if}
        </div>
      </div>
    </div>
  {/if}
  {#if serversOpen}
    <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
    <div class="acc-scrim" onclick={() => { if (!srvBusy) serversOpen = false; }}></div>
    <div class="acc-sheet srv-sheet" role="dialog" aria-label={$t("Server")}>
      <div class="acc-grab"></div>
      <div class="acc-title">{$t("Server")}</div>
      <div class="srv-now">
        <span class="srv-dot" class:ok={$info?.connected}></span>
        <span class="srv-addr">{hostOf($info?.server_url ?? null) || "—"}</span>
        <span class="srv-st">{$info?.connected ? ($ping != null ? `${$ping} ${$t("ms")}` : $t("online")) : $t("Connecting…")}</span>
      </div>
      <div class="srv-pick"><ServerPicker bind:value={srvUrl} inline placeholder="http://…:7777" /></div>
      {#if srvErr}<div class="srv-err">{srvErr}</div>{/if}
      <button class="srv-go" disabled={srvBusy || !srvUrl.trim()} onclick={reconnectTo}>
        {srvBusy ? $t("Connecting…") : $t("Connect to this server")}
      </button>
      <div class="acc-note">{$t("Your account lives on the server it was created on; another server works only if they're in one federation.")}</div>
    </div>
  {/if}
  {#if accountsOpen}
    <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
    <div class="acc-scrim" onclick={() => { if (!switching) accountsOpen = false; }}></div>
    <div class="acc-sheet" role="dialog" aria-label={$t("Accounts")}>
      <div class="acc-grab"></div>
      <div class="acc-title">{$t("Accounts")}</div>
      {#each $accounts as a (a.slot)}
        <div class="acc-row" class:on={a.active}>
          <button class="acc-pick" disabled={switching} onclick={() => { if (a.active) { accountsOpen = false; openProfile(); } else void pickAccount(a.slot); }}>
            <span class="acc-ava">
              {#if a.active && $profile.avatar_url}<img src={$profile.avatar_url} alt="" />{:else}{(a.username || "?").charAt(0).toUpperCase()}{/if}
            </span>
            <span class="acc-who">
              <span class="acc-name">{a.username ? "@" + a.username : $t("Not signed in")}</span>
              <span class="acc-sub">{hostOf(a.server_url) || $t("Sign-in not finished")}</span>
            </span>
            {#if a.active}<span class="acc-tick">{@html icons.check}</span>{/if}
          </button>
          {#if !a.active && !a.registered && a.slot}
            <button class="acc-x" onclick={() => forgetAccount(a.slot)} aria-label={$t("Remove")} title={$t("Remove")}>×</button>
          {/if}
        </div>
      {/each}
      <button class="acc-add" disabled={switching} onclick={() => pickAccount("new")}>
        <span class="acc-plus">+</span>{$t("Add account")}
      </button>
      {#if switching}<div class="acc-note">{$t("Opening the account…")}</div>
      {:else}<div class="acc-note">{$t("Each account keeps its own chats on this device. The app restarts to switch.")}</div>{/if}
    </div>
  {/if}
</aside>

<style>
  .rp-veil { position: fixed; inset: 0; z-index: 140; }
  .rp { position: fixed; z-index: 141; width: 260px; max-height: 70vh; overflow-y: auto; padding: 6px;
    background: color-mix(in srgb, var(--accent) 4%, var(--bg-shell)); border: 1px solid var(--border-strong);
    border-radius: calc(14px * var(--rk, 1)); box-shadow: 0 22px 50px -16px rgba(0,0,0,.7); animation: rp-in .14s ease-out; }
  @keyframes rp-in { from { opacity: 0; transform: scale(.96); } }
  .rp-title { font-weight: 800; font-size: 14px; padding: 6px 8px 4px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .rp-sec { font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: .06em; color: var(--fg-dim); padding: 8px 8px 2px; }
  .rp-row { width: 100%; display: flex; align-items: center; gap: 10px; padding: 7px 8px; border: none; background: none; color: var(--fg);
    border-radius: calc(9px * var(--rk, 1)); font: inherit; font-size: 13.5px; text-align: left; cursor: pointer; }
  .rp-row:hover, .rp-row:focus-visible { background: var(--accent-soft); outline: none; }
  .rp-ic { width: 22px; text-align: center; color: var(--fg-muted); font-weight: 700; flex-shrink: 0; }
  .rp-name { flex: 1; min-width: 0; display: flex; flex-direction: column; overflow: hidden; }
  .rp-name small { color: var(--fg-dim); font-size: 11.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .rp-tag { font-size: 10.5px; color: var(--fg-dim); border: 1px solid var(--border); border-radius: 6px; padding: 0 5px; }
  .rp-empty { padding: 10px 8px; color: var(--fg-dim); font-size: 13px; }
  .avatar.lk { filter: saturate(0.3); }
  .lk-ic { position: absolute; inset: 0; display: grid; place-items: center; color: rgba(255,255,255,0.9); }
  .lk-ic :global(svg) { width: 18px; height: 18px; }
  .lk-blur { filter: blur(3px); letter-spacing: 2px; user-select: none; }
  .lk-prev { color: var(--fg-dim); font-style: italic; }
  .ctx-kbd { float: right; margin-left: 12px; font-size: 11px; color: var(--fg-dim); }
  .chat-item.kb { box-shadow: inset 0 0 0 2px var(--accent); }
  .resize-handle { position: absolute; top: 0; right: -3px; width: 6px; height: 100%; cursor: col-resize; z-index: 6; }
  .resize-handle::after { content: ""; position: absolute; left: 2px; top: 0; bottom: 0; width: 2px; background: transparent; transition: background .2s; }
  .resize-handle:hover::after, .list.resizing .resize-handle::after { background: var(--accent); }
  .list.resizing { user-select: none; }
  .pin-ic { width: 14px; height: 14px; color: var(--fg-dim); flex-shrink: 0; transform: rotate(45deg); }
  .pin-ic svg { width: 100%; height: 100%; }
  .chat-item.pinned { background: color-mix(in srgb, var(--bg-hover) 50%, transparent); }
  .chat-item.dragging { opacity: .6; transform: scale(.98); cursor: grabbing; }
  .chat-item.drop-before { box-shadow: inset 0 3px 0 var(--accent); }
  .cn-text { overflow: hidden; text-overflow: ellipsis; min-width: 0; }
  .muted-ic { display: inline-grid; color: var(--fg-dim); flex-shrink: 0; }
  .muted-ic svg { width: 13px; height: 13px; }
  .arch-ic {
    width: 44px; height: 44px; border-radius: 50%; display: grid; place-items: center;
    background: var(--bg-input); color: var(--fg-muted); border: 1px solid var(--border);
  }
  .arch-ic svg { width: 21px; height: 21px; }
  .arch-back {
    display: flex; align-items: center; gap: 8px; width: 100%; padding: 8px 10px 10px;
    background: none; border: none; color: var(--fg); font: inherit; font-weight: 800; font-size: 14px; cursor: pointer;
  }
  .arch-back svg { width: 18px; height: 18px; color: var(--accent); }
  .arch-back:hover { color: var(--accent); }
  .join-card {
    display: flex; align-items: center; gap: 11px; margin: 4px 6px 8px; padding: 10px;
    border-radius: calc(12px * var(--rk, 1)); border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent);
    background: color-mix(in srgb, var(--accent) 7%, transparent);
  }
  .jc-ava {
    width: 40px; height: 40px; border-radius: var(--ava-r, 50%); flex-shrink: 0; display: grid; place-items: center;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070C; font-weight: 800;
  }
  .jc-body { flex: 1; min-width: 0; }
  .jc-title { font-weight: 700; font-size: 13.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .jc-sub { font-size: 11.5px; color: var(--fg-dim); }
  .jc-btn { border: none; border-radius: calc(8px * var(--rk, 1)); padding: 7px 11px; background: var(--accent); color: #06070C; font: inherit; font-size: 12px; font-weight: 800; cursor: pointer; white-space: nowrap; }
  .jc-done { font-size: 12px; color: var(--fg-muted); font-weight: 700; }
  .ctx-scrim { position: fixed; inset: 0; z-index: 70; }
  /* chat groups */
  .gstack { position: relative; width: 44px; height: 44px; flex-shrink: 0; }
  .gmini { position: absolute; width: 28px; height: 28px; font-size: 11px; border: 2px solid var(--bg-list);
    left: calc(var(--k) * 8px); top: calc(var(--k) * 8px); z-index: calc(3 - var(--k)); }
  .gemo { display: grid; place-items: center; width: 44px; height: 44px; border-radius: calc(14px * var(--rk, 1)); background: var(--bg-input); font-size: 22px; }
  .g-count { flex-shrink: 0; min-width: 18px; padding: 0 6px; height: 17px; border-radius: calc(9px * var(--rk, 1)); font-size: 10.5px; font-weight: 800; line-height: 17px; text-align: center;
    color: var(--fg-muted); background: var(--bg-input); border: 1px solid var(--border); }
  .chat-item.grp .cn-text { font-weight: 700; }
  .g-tag { flex-shrink: 1; min-width: 0; max-width: 46%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; padding: 1px 7px; border-radius: calc(7px * var(--rk, 1));
    font-size: 10.5px; font-weight: 700; color: var(--accent); background: var(--accent-soft); cursor: pointer; }
  .g-tag:hover { filter: brightness(1.2); }
  /* Peek (hold an avatar) */
  .peek-veil {
    position: fixed; inset: 0; z-index: 90;
    display: grid; place-items: center; padding: 16px;
    background: rgba(0, 0, 0, 0.35);
    backdrop-filter: blur(14px) saturate(1.2); -webkit-backdrop-filter: blur(14px) saturate(1.2);
    animation: peek-fade 0.18s ease-out;
  }
  @keyframes acc-fade { from { opacity: 0; } }
  @keyframes m-spin { to { transform: rotate(360deg); } }
  @keyframes acc-up { from { transform: translateY(40px); opacity: 0; } }
  @keyframes peek-fade { from { opacity: 0; } }
  .peek {
    width: min(420px, 100%); max-height: calc(100vh - 32px);
    display: flex; flex-direction: column; gap: 10px;
    animation: peek-pop 0.26s cubic-bezier(0.2, 0.9, 0.25, 1.12);
  }
  @keyframes peek-pop { from { opacity: 0; transform: scale(0.88); } to { opacity: 1; transform: scale(1); } }
  .chat-badge.mark { min-width: 12px; width: 12px; height: 12px; padding: 0; }
  .peek.grow { animation: peek-grow .24s cubic-bezier(.3,.7,.2,1) forwards; }
  .peek.grow .peek-menu { opacity: 0; transition: opacity .12s; }
  @keyframes peek-grow { to { transform: scale(1.18); opacity: 0; } }
  .peek-card {
    display: flex; flex-direction: column; min-height: 0; flex: 1 1 auto;
    padding: 0; border: 1px solid var(--border-strong); border-radius: calc(18px * var(--rk, 1)); overflow: hidden;
    background: var(--bg-chat, var(--bg-surface)); color: var(--fg); font: inherit; text-align: left; cursor: pointer;
    box-shadow: 0 24px 60px -18px rgba(0, 0, 0, 0.7);
  }
  .peek-head {
    display: flex; align-items: center; gap: 10px; padding: 10px 12px;
    background: color-mix(in srgb, var(--bg-shell) 85%, transparent); border-bottom: 1px solid var(--border);
  }
  .peek-who { min-width: 0; }
  .peek-name { font-weight: 700; font-size: 15px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .peek-st { font-size: 12px; color: var(--fg-dim); }
  .peek-msgs {
    height: min(52vh, 420px); overflow-y: auto; padding: 10px 10px 12px;
    display: flex; flex-direction: column; gap: 4px; scrollbar-width: none;
  }
  .peek-empty, .peek-sys { align-self: center; font-size: 12px; color: var(--fg-dim); padding: 4px 10px; border-radius: calc(10px * var(--rk, 1)); background: var(--bg-input); }
  .peek-empty { margin: auto; }
  .peek-row { display: flex; }
  .peek-row.mine { justify-content: flex-end; }
  .peek-bubble {
    position: relative; max-width: 80%; padding: 6px 10px 6px; border-radius: calc(14px * var(--rk, 1));
    background: var(--bubble-in, var(--bg-surface-2)); font-size: 13.5px; line-height: 1.35;
    display: flex; flex-wrap: wrap; align-items: flex-end; gap: 0 8px;
  }
  .peek-row.mine .peek-bubble { background: var(--bubble-out, color-mix(in srgb, var(--accent) 30%, var(--bg-surface-2))); }
  .peek-bubble.pic { padding: 3px; flex-direction: column; align-items: stretch; }
  .peek-anim { display: block; width: 120px; height: 120px; }
  .peek-bubble.pic img { display: block; max-width: 100%; max-height: 180px; border-radius: calc(11px * var(--rk, 1)); object-fit: cover; }
  .peek-bubble.pic .peek-time { position: absolute; right: 8px; bottom: 6px; padding: 0 5px; border-radius: calc(8px * var(--rk, 1)); background: rgba(0,0,0,0.45); color: #fff; }
  .peek-txt { white-space: pre-wrap; word-break: break-word; display: -webkit-box; -webkit-line-clamp: 8; line-clamp: 8; -webkit-box-orient: vertical; overflow: hidden; }
  .peek-txt.code { font-family: var(--mono, monospace); font-size: 12px; }
  .peek-time { margin-left: auto; font-size: 10.5px; color: var(--fg-dim); }
  .peek-menu {
    align-self: flex-start; min-width: 210px; padding: 5px;
    display: flex; flex-direction: column; gap: 1px;
    background: var(--bg-shell); border: 1px solid var(--border-strong); border-radius: calc(14px * var(--rk, 1));
    box-shadow: 0 18px 44px -14px rgba(0,0,0,0.6);
    animation: peek-pop 0.3s cubic-bezier(0.2, 0.9, 0.25, 1.12) 0.03s both;
  }
  .peek-menu button {
    text-align: left; padding: 11px 14px; border-radius: calc(9px * var(--rk, 1)); border: none; background: transparent;
    color: var(--fg); font: inherit; font-size: 14px; cursor: pointer;
  }
  .peek-menu button:hover { background: var(--accent-soft); color: var(--accent); }
  .chat-ctx {
    position: fixed; z-index: 71; min-width: 180px; padding: 5px;
    background: var(--bg-shell); border: 1px solid var(--border-strong); border-radius: calc(12px * var(--rk, 1));
    box-shadow: 0 18px 44px -14px rgba(0,0,0,0.6); display: flex; flex-direction: column; gap: 2px;
  }
  .chat-ctx button {
    text-align: left; padding: 8px 12px; border-radius: calc(8px * var(--rk, 1)); border: none; background: transparent;
    color: var(--fg); font: inherit; font-size: 13.5px; cursor: pointer;
  }
  .chat-ctx button:hover { background: var(--accent-soft); color: var(--accent); }
  .chat-ctx button.danger, .peek-menu button.danger { color: var(--danger, #FF4D6D); }
  .chat-ctx button.danger:hover, .peek-menu button.danger:hover { background: color-mix(in srgb, var(--danger, #FF4D6D) 14%, transparent); color: var(--danger, #FF4D6D); }
  .chat-preview.activity { color: var(--accent); }
  .chat-preview.activity b { font-weight: 700; }
  /* Compact: avatars only (a server is open). */
  .list.compact .list-header, .list.compact .folder-strip, .list.compact .chat-body,
  .list.compact .resize-handle, .list.compact .join-card { display: none; }
  .list.compact .chats { padding: 6px 0; scrollbar-width: none; }
  .list.compact .chat-item { position: relative; justify-content: center; padding: 6px 0; margin: 2px 8px; border-radius: calc(16px * var(--rk, 1)); }
  .list.compact .chat-item.active { margin: 2px 0 2px 8px; width: auto; border-radius: 16px 0 0 16px; }
  .compact-expand { margin: 12px auto 4px; width: 44px; height: 36px; border-radius: calc(12px * var(--rk, 1)); border: 1px solid var(--border); background: var(--bg-input); color: var(--fg-muted); cursor: pointer; display: grid; place-items: center; flex-shrink: 0; transition: color .15s, border-color .15s; }
  .compact-expand:hover { color: var(--accent); border-color: color-mix(in srgb, var(--accent) 40%, transparent); }
  .compact-expand svg { width: 18px; height: 18px; }
  .collapse-btn { width: 30px; height: 36px; flex-shrink: 0; border: none; border-radius: calc(10px * var(--rk, 1)); background: none; color: var(--fg-muted); cursor: pointer; display: grid; place-items: center; }
  .collapse-btn:hover { background: var(--bg-hover); color: var(--accent); }
  .collapse-btn svg { width: 16px; height: 16px; }
  .cmp-badge { position: absolute; right: -4px; bottom: -4px; z-index: 2; min-width: 18px; height: 18px; padding: 0 5px; box-sizing: border-box; border-radius: calc(9px * var(--rk, 1)); background: #F23F43; color: #fff; font-size: 11px; font-weight: 800; line-height: 18px; text-align: center; border: 2px solid var(--bg-list); }
  .cmp-badge.muted { background: var(--fg-dim); }
  .list {
    position: relative;
    background: var(--bg-list);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .list-header {
    container-type: inline-size;
    padding: 14px 14px 10px;
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .search {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: calc(10px * var(--rk, 1));
    transition: border-color 0.15s ease, box-shadow 0.15s ease;
  }
  .search:focus-within {
    border-color: var(--accent-soft);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .search :global(svg) { width: 15px; height: 15px; color: var(--fg-dim); flex-shrink: 0; }
  .search input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--fg);
    font-family: inherit;
    font-size: 13.5px;
    min-width: 0;
  }
  .search input::placeholder { color: var(--fg-dim); }
  .search-kbd {
    font-family: var(--font-mono);
    font-size: 10.5px;
    color: var(--fg-dim);
    background: rgba(255,255,255,0.04);
    padding: 2px 5px;
    border-radius: calc(4px * var(--rk, 1));
    border: 1px solid var(--border);
  }
  /* Small square "+" button next to the search bar. Pulses while
     the user has no real chats yet so it reads as the primary
     action, otherwise it sits quietly. */
  .plus-wrap { position: relative; flex-shrink: 0; }
  .plus-pop {
    position: absolute; top: 42px; right: 0; z-index: 40;
    background: var(--bg-shell); border: 1px solid var(--border-strong);
    border-radius: calc(12px * var(--rk, 1)); padding: 5px; min-width: 160px;
    box-shadow: 0 18px 44px -14px rgba(0,0,0,0.6);
    display: flex; flex-direction: column; gap: 2px;
  }
  .plus-pop button {
    text-align: left; padding: 9px 12px; border-radius: calc(8px * var(--rk, 1)); border: none;
    background: transparent; color: var(--fg); font: inherit; font-size: 13.5px; font-weight: 500; cursor: pointer;
  }
  .plus-pop button:hover { background: var(--accent-soft); color: var(--accent); }
  .plus-scrim { position: fixed; inset: 0; z-index: 30; }
  .plus-btn {
    width: 34px; height: 34px;
    display: grid; place-items: center;
    background: linear-gradient(135deg, var(--accent) 0%, var(--accent-2) 100%);
    color: #06070C;
    border: none;
    border-radius: calc(10px * var(--rk, 1));
    cursor: pointer;
    flex-shrink: 0;
    box-shadow: 0 4px 14px -6px var(--accent-glow);
    transition: all 0.15s ease;
  }
  .plus-btn:hover {
    transform: translateY(-1px);
    box-shadow: 0 6px 18px -4px var(--accent-glow);
  }
  .plus-glyph { width: 18px; height: 18px; display: block; }
  .search input { min-width: 0; }
  @container (max-width: 300px) { .search-kbd { display: none; } }
  .plus-btn.nudge {
    animation: plus-pulse 1.8s ease-in-out infinite;
  }
  @keyframes plus-pulse {
    0%, 100% { box-shadow: 0 4px 14px -6px var(--accent-glow), 0 0 0 0 var(--accent-glow); }
    50%      { box-shadow: 0 4px 14px -6px var(--accent-glow), 0 0 0 6px color-mix(in srgb, var(--accent) 0%, transparent); }
  }

  /* "Start chat with …" row shown at the top of the list when the
     search term doesn't match an existing chat. One click opens the
     new-chat dialog with the typed name pre-filled. */
  .dir-head {
    font-size: 10.5px;
    font-weight: 700;
    color: var(--fg-dim);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    padding: 8px 12px 4px;
  }
  .dir-row {
    display: grid;
    grid-template-columns: 36px 1fr auto;
    gap: 11px;
    align-items: center;
    width: calc(100% - 12px);
    margin: 0 6px 2px;
    padding: 8px 10px;
    border-radius: calc(10px * var(--rk, 1));
    background: transparent;
    border: none;
    text-align: left;
    color: inherit;
    cursor: pointer;
  }
  .dir-row:hover { background: var(--bg-hover); }
  .dir-avatar {
    width: 36px; height: 36px;
    border-radius: var(--ava-r, 50%);
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    display: grid; place-items: center;
    font-weight: 800; font-size: 14px; color: #06070C;
  }
  .dir-body { min-width: 0; }
  .dir-name { font-weight: 600; font-size: 13.5px; display: flex; align-items: center; gap: 6px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bot-tag { font-size: 9px; font-weight: 800; background: var(--accent-soft); color: var(--accent); padding: 1px 5px; border-radius: calc(4px * var(--rk, 1)); letter-spacing: 0.05em; }
  .dir-sub { font-size: 11.5px; color: var(--fg-dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dir-go { font-size: 20px; font-weight: 700; color: var(--accent); padding-right: 6px; }

  .start-row {
    display: grid;
    grid-template-columns: 44px 1fr;
    gap: 12px;
    padding: 10px 10px;
    margin: 4px 6px 8px;
    border-radius: calc(10px * var(--rk, 1));
    cursor: pointer;
    background: linear-gradient(135deg, color-mix(in srgb, var(--accent) 8%, transparent), color-mix(in srgb, var(--accent-2) 6%, transparent));
    border: 1px dashed color-mix(in srgb, var(--accent) 35%, transparent);
    text-align: left;
    color: var(--fg);
    width: calc(100% - 12px);
    transition: all 0.12s ease;
    align-items: center;
  }
  .start-row:hover {
    background: linear-gradient(135deg, color-mix(in srgb, var(--accent) 14%, transparent), color-mix(in srgb, var(--accent-2) 10%, transparent));
    border-style: solid;
  }
  .start-icon {
    width: 44px; height: 44px;
    border-radius: calc(12px * var(--rk, 1));
    background: linear-gradient(135deg, var(--accent) 0%, var(--accent-2) 100%);
    color: #06070C;
    display: grid; place-items: center;
    flex-shrink: 0;
  }
  .start-icon :global(svg) { width: 18px; height: 18px; }
  .start-title { font-size: 13.5px; font-weight: 500; color: var(--fg); }
  .start-title :global(b) { color: var(--accent); }
  .start-sub   { font-size: 11.5px; color: var(--fg-dim); margin-top: 2px; }

  /* Folder strip (Telegram-style) */
  .folder-strip {
    display: flex; align-items: center; gap: 4px;
    padding: 0 10px 8px;
    overflow-x: auto;
    border-bottom: 1px solid var(--border);
  }
  .folder-strip.empty-strip { padding-bottom: 8px; }
  .folder-tab {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 5px 12px; border-radius: 100px;
    background: transparent; border: 1px solid transparent;
    color: var(--fg-muted); font-size: 12.5px; font-weight: 600;
    cursor: pointer; flex-shrink: 0; white-space: nowrap;
  }
  .folder-tab:hover { color: var(--fg); background: var(--bg-hover); }
  .folder-tab.active { color: var(--accent); background: var(--accent-soft); border-color: color-mix(in srgb, var(--accent) 25%, transparent); }
  .ft-count {
    font-size: 10px; font-variant-numeric: tabular-nums;
    background: var(--bg-input); color: var(--fg-dim);
    border-radius: 100px; padding: 0 5px;
  }
  .folder-tab.active .ft-count { background: color-mix(in srgb, var(--accent) 20%, transparent); color: var(--accent); }
  .folder-edit {
    flex-shrink: 0;
    padding: 5px 10px; border-radius: 100px;
    background: transparent; border: 1px dashed var(--border-strong);
    color: var(--fg-dim); font-size: 12px; font-weight: 600; cursor: pointer;
    white-space: nowrap;
  }
  .folder-edit:hover { color: var(--accent); border-color: var(--accent-soft); }

  .chats {
    flex: 1;
    overflow-y: auto;
    padding: 4px 6px 6px;
  }

  .chat-item {
    display: grid;
    grid-template-columns: 44px 1fr;
    gap: 12px;
    padding: 10px 10px;
    border-radius: calc(10px * var(--rk, 1));
    cursor: pointer;
    transition: background-color 0.2s var(--ease-out, ease), box-shadow 0.25s var(--ease-out, ease), transform 0.2s var(--ease-out, ease);
    align-items: center;
    min-width: 0;
    width: 100%;
    background: transparent;
    border: none;
    text-align: left;
    color: inherit;
  }
  .chat-item:hover { background: var(--bg-hover); }
  /* The open chat is a notebook tab: its row runs into the list's edge,
     which becomes the spine the chat panel hangs on. */
  .list { --tab: color-mix(in srgb, var(--accent) 15%, var(--bg-list)); }
  .list.has-active { border-right-color: color-mix(in srgb, var(--accent) 42%, var(--border)); }
  .chat-item.active {
    position: relative; z-index: 1;
    background: var(--tab);
    width: calc(100% + 6px); margin-right: -6px;
    border-radius: 12px 0 0 12px;
  }
  .chat-item.active::before, .chat-item.active::after {
    content: ""; position: absolute; right: 0; width: 12px; height: 12px; pointer-events: none;
  }
  .chat-item.active::before { top: -12px; background: radial-gradient(circle at 0 0, transparent 11.5px, var(--tab) 12px); }
  .chat-item.active::after { bottom: -12px; background: radial-gradient(circle at 0 100%, transparent 11.5px, var(--tab) 12px); }
  .chat-body { min-width: 0; }
  .chat-row-1 {
    display: flex; align-items: center; gap: 6px;
    justify-content: space-between;
  }
  .chat-name {
    font-weight: 600;
    font-size: calc(14.5px * var(--font-scale, 1));
    color: var(--fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .chat-time { font-size: 11.5px; color: var(--fg-dim); flex-shrink: 0; font-variant-numeric: tabular-nums; }
  .chat-item.active .chat-time { color: var(--accent); }
  .chat-row-2 {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 3px;
    justify-content: space-between;
  }
  .chat-preview {
    font-size: calc(13px * var(--font-scale, 1));
    color: var(--fg-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    flex: 1;
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .lock-mini { display: inline-flex; align-items: center; }
  .lock-mini :global(svg) { width: 11px; height: 11px; color: var(--accent); opacity: 0.7; flex-shrink: 0; }
  .me-prefix { color: var(--fg-dim); flex-shrink: 0; }
  .chat-preview.typing { color: var(--accent); }
  .chat-badge {
    background: var(--accent);
    color: #06070C;
    font-weight: 700;
    font-size: 11px;
    padding: 2px 7px;
    border-radius: calc(10px * var(--rk, 1));
    min-width: 18px;
    text-align: center;
    box-shadow: 0 0 12px -2px var(--accent-glow);
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
  }
  .chat-badge.muted {
    background: var(--fg-dim);
    color: var(--fg);
    box-shadow: none;
  }

  .session-info {
    padding: 10px 14px;
    border-top: 1px solid var(--border);
    background: rgba(0,0,0,0.15);
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 11.5px;
    color: var(--fg-dim);
  }
  .session-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .session-fp {
    font-family: var(--font-mono);
    color: var(--fg-muted);
    letter-spacing: 0.02em;
  }
  .copy-fp {
    background: transparent;
    border: none;
    color: var(--fg-dim);
    cursor: pointer;
    padding: 2px;
    display: grid; place-items: center;
    border-radius: calc(4px * var(--rk, 1));
  }
  .copy-fp:hover { color: var(--accent); background: var(--bg-hover); }
  .copy-fp :global(svg) { width: 12px; height: 12px; }
  .data-dir .dir-path {
    direction: rtl;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 220px;
    font-size: 10.5px;
  }
  .accent-dot {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 6px var(--accent-glow);
    display: inline-block;
    margin-right: 4px;
    animation: pulse 2s ease-in-out infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50%      { opacity: 0.5; transform: scale(0.85); }
  }
  .verified-badge :global(svg) { width: 13px; height: 13px; color: var(--accent); filter: drop-shadow(0 0 4px var(--accent-glow)); }
  @media (max-width: 720px) { .search-kbd { display: none; } }

  /* ---- Phones: the main page as in the new Telegram ---- */
  .m-title, .acc-scrim, .acc-sheet { display: none; }
  @media (max-width: 720px) {
    /* One column: no edge between the list and a chat beside it (it showed
       as a teal line down the right side once a chat had been opened). */
    .list, .list.has-active { border-right: none; }
    .m-title { display: flex; align-items: center; gap: 9px; padding: 10px 10px 6px 16px; }
    .m-me { width: 30px; height: 30px; flex-shrink: 0; border-radius: 50%; border: none; padding: 0; overflow: hidden; position: relative;
      display: grid; place-items: center; font-weight: 800; font-size: 13px; color: #06070c;
      background: linear-gradient(135deg, var(--accent), var(--accent-2)); touch-action: manipulation;
      -webkit-touch-callout: none; user-select: none; -webkit-user-select: none; }
    .m-me img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; pointer-events: none; }
    .m-net { flex-shrink: 0; display: inline-flex; align-items: center; gap: 6px; white-space: nowrap; border: none; cursor: pointer;
      font: 700 11px var(--font-ui); font-variant-numeric: tabular-nums; color: var(--online, #35D399);
      padding: 2px 8px; border-radius: calc(9px * var(--rk, 1)); background: color-mix(in srgb, currentColor 13%, transparent); align-self: center; }
    .m-net.slow { color: #F0B232; }
    .m-net.bad { color: var(--danger, #FF4D6D); }
    .m-net.off { color: var(--fg-muted); font-weight: 600; }
    .m-dot { width: 7px; height: 7px; border-radius: 50%; background: currentColor; }
    .m-spin { width: 10px; height: 10px; border-radius: 50%; border: 2px solid currentColor; border-right-color: transparent; animation: m-spin .8s linear infinite; }
    /* The servers sheet */
    .srv-sheet { max-height: calc(100dvh - 24px - env(safe-area-inset-top, 0px)); overflow-y: auto; }
    .srv-now { display: flex; align-items: center; gap: 10px; padding: 8px 12px; border-radius: calc(14px * var(--rk, 1)); background: var(--accent-soft); }
    .srv-dot { width: 9px; height: 9px; border-radius: 50%; background: var(--fg-dim); flex-shrink: 0; }
    .srv-dot.ok { background: var(--online, #35D399); box-shadow: 0 0 8px var(--online, #35D399); }
    .srv-addr { flex: 1; min-width: 0; font-weight: 700; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .srv-st { font-size: 13px; color: var(--fg-muted); font-variant-numeric: tabular-nums; }
    .srv-pick { padding: 8px 4px 4px; }
    .srv-err { margin: 4px 8px; padding: 8px 10px; border-radius: calc(10px * var(--rk, 1)); font-size: 13px; color: var(--danger, #FF4D6D); background: color-mix(in srgb, var(--danger, #FF4D6D) 12%, transparent); overflow-wrap: anywhere; }
    .srv-go { margin: 6px 8px 0; padding: 12px; border: none; border-radius: calc(14px * var(--rk, 1)); font: 700 15px var(--font-ui); color: #06070c;
      background: linear-gradient(135deg, var(--accent), var(--accent-2)); }
    .srv-go:disabled { opacity: .6; }
    /* Accounts: a sheet from the bottom. */
    .acc-scrim { display: block; position: fixed; inset: 0; z-index: 80; background: rgba(0,0,0,.35); animation: acc-fade .2s ease; }
    .acc-sheet { display: flex; flex-direction: column; gap: 2px; position: fixed; left: 8px; right: 8px; z-index: 81;
      bottom: calc(8px + env(safe-area-inset-bottom, 0px)); padding: 6px 8px 12px; border-radius: calc(26px * var(--rk, 1));
      background: color-mix(in srgb, var(--bg-shell) var(--menu-alpha, 82%), transparent);
      backdrop-filter: blur(24px) saturate(1.5); -webkit-backdrop-filter: blur(24px) saturate(1.5);
      border: 1px solid var(--border-strong); box-shadow: 0 20px 50px -20px rgba(0,0,0,.8);
      animation: acc-up .3s cubic-bezier(.2,.8,.2,1); }
    .acc-grab { width: 38px; height: 4px; border-radius: 2px; background: var(--border-strong); margin: 4px auto 6px; }
    .acc-title { font-size: 13px; font-weight: 700; color: var(--fg-dim); text-transform: uppercase; letter-spacing: .06em; padding: 4px 12px 6px; }
    .acc-row { display: flex; align-items: center; gap: 4px; border-radius: calc(16px * var(--rk, 1)); }
    .acc-row.on { background: var(--accent-soft); }
    .acc-pick { flex: 1; min-width: 0; display: flex; align-items: center; gap: 12px; padding: 10px 12px; border: none; background: transparent; color: var(--fg); text-align: left; border-radius: calc(16px * var(--rk, 1)); }
    .acc-ava { width: 42px; height: 42px; border-radius: var(--ava-r, 50%); flex-shrink: 0; position: relative; overflow: hidden; display: grid; place-items: center;
      font-weight: 800; color: #06070c; background: linear-gradient(135deg, var(--accent), var(--accent-2)); }
    .acc-ava img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
    .acc-who { display: flex; flex-direction: column; min-width: 0; flex: 1; }
    .acc-name { font-size: 16px; font-weight: 700; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .acc-sub { font-size: 13px; color: var(--fg-dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .acc-tick { color: var(--accent); display: grid; }
    .acc-tick :global(svg) { width: 20px; height: 20px; }
    .acc-x { width: 40px; height: 40px; border: none; background: transparent; color: var(--fg-dim); font-size: 22px; border-radius: 50%; }
    .acc-add { display: flex; align-items: center; gap: 12px; padding: 10px 12px; border: none; background: transparent; color: var(--accent); font: 600 16px var(--font-ui); border-radius: calc(16px * var(--rk, 1)); text-align: left; }
    .acc-plus { width: 42px; height: 42px; border-radius: 50%; display: grid; place-items: center; font-size: 24px; background: var(--accent-soft); }
    .acc-note { font-size: 12.5px; color: var(--fg-dim); padding: 6px 12px 0; line-height: 1.4; }
    .m-name { font-size: 17px; font-weight: 700; letter-spacing: -0.01em; font-family: var(--font-display, inherit); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .m-menu { margin-left: auto; width: 34px; height: 34px; border-radius: 50%; border: none; background: transparent; color: var(--fg); display: grid; place-items: center; }
    .m-menu svg { width: 18px; height: 18px; }
    .m-menu:active { background: var(--bg-hover); }
    .list-header { padding: 2px 14px 8px; }
    .plus-btn { display: none; }
    .plus-pop { position: fixed; top: calc(env(safe-area-inset-top, 0px) + 46px); right: 14px; min-width: 200px; border-radius: calc(16px * var(--rk, 1)); padding: 6px; }
    .plus-pop button { padding: 12px 14px; font-size: 15px; border-radius: calc(11px * var(--rk, 1)); }
    .search { height: 36px; padding: 0 12px; border-radius: calc(18px * var(--rk, 1)); background: var(--bg-surface-2, var(--bg-input)); border-color: transparent; gap: 8px; }
    .search :global(svg) { width: 15px; height: 15px; }
    .search input { font-size: 14px; }
    /* Folders: a pill tray, the current one lit. */
    .folder-strip { margin: 0; padding: 0 14px 8px; gap: 4px; border: none; border-bottom: 1px solid var(--border); border-radius: 0;
      background: transparent; scrollbar-width: none; }
    .folder-strip.empty-strip { padding-bottom: 8px; }
    .folder-strip.empty-strip .folder-tab { flex: 0 0 auto; }
    .folder-tab { flex: 0 0 auto; justify-content: center; padding: 5px 11px; font-size: 12.5px; font-weight: 600; border: 1px solid transparent; border-radius: calc(7px * var(--rk, 1)); color: var(--fg-muted); }
    .folder-tab.active { background: var(--accent-soft); color: var(--accent); border-color: color-mix(in srgb, var(--accent) 25%, transparent); }
    .folder-edit { padding: 5px 10px; font-size: 12.5px; border: none; }
    /* Rows: big avatars, roomy text, no notebook tab (one panel at a time). */
    .chats { padding: 4px 6px calc(68px + env(safe-area-inset-bottom, 0px)); }
    .chat-item { grid-template-columns: 48px 1fr; gap: 12px; padding: 7px 10px; border-radius: calc(16px * var(--rk, 1)); }
    .chat-item .avatar.sz-44 { width: 48px; height: 48px; font-size: 18px; }
    .chat-item.active { background: transparent; width: 100%; margin-right: 0; border-radius: calc(16px * var(--rk, 1)); }
    .chat-item.active::before, .chat-item.active::after { display: none; }
    .chat-item.active .chat-time { color: var(--fg-dim); }
    .chat-name { font-size: calc(15px * var(--font-scale, 1)); }
    .chat-preview { font-size: calc(13.5px * var(--font-scale, 1)); }
    .chat-time { font-size: 12px; }
    .chat-row-1 { margin-bottom: 2px; }

  }
</style>
