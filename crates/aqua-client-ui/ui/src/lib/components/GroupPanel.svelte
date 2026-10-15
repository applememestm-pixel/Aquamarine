<script module lang="ts">
  import { writable, get } from "svelte/store";
  import { peerPresence } from "../presence";
  export const groupPanelId = writable<string | null>(null);
  export function openGroupPanel(gid: string) { groupPanelId.set(gid); }
  export function closeGroupPanel() { groupPanelId.set(null); }
</script>

<script lang="ts">
  import { t, tr, tn } from "../i18n";
  /**
   * Telegram-grade group / channel info and management:
   *   info → (admins) manage → type & link, permissions, reactions,
   *   administrators (rights + custom titles), members (tags, restrict, ban),
   *   removed users, join requests, server tag (Discord), recent actions.
   * Toggles apply immediately (like Telegram); text fields have a Save.
   */
  import { groupMap, groupMedia, refreshGroups } from "../groups";
  import { openFileDialog } from "../dialog";
  import { cropImage } from "./ImageCropper.svelte";
  import { askConfirm } from "./Confirm.svelte";
  import { activeChat, activeMessages } from "../stores";
  import type { ChatMessage } from "../types";
  import { info, selectChat, backToList } from "../stores";
  import { api } from "../tauri";
  import { showToast } from "./Toast.svelte";
  import { profile, updateProfile } from "../profile";
  import { publishMyProfile } from "../people";
  import { mutedChats, setMuted } from "../chatPrefs";
  import { reactionPack, quickReactions, reactionUrl, parseReaction } from "../reactionPack";
  import { CLAN_BADGES, CLAN_COLORS, badgeSvg } from "../clanBadges";
  import { memberOf, isAdmin, isOwner, can, tagOf, shareLink, ALL_RIGHTS, DEFAULT_ROOMS } from "../groupRules";
  import type { Room } from "../types";
  import AnimatedMedia from "./AnimatedMedia.svelte";
  import ClanTag from "./ClanTag.svelte";
  import type { AdminRights, Group, GroupMember, Perm } from "../types";

  type Page = "rooms" | "info" | "manage" | "type" | "perms" | "reactions" | "admins" | "admin-edit" | "members" | "banned" | "requests" | "clan" | "log";
  let page = $state<Page>("info");
  let stack: Page[] = [];
  let editUser = $state("");

  let me = $derived($info?.username ?? "");
  let group = $derived($groupPanelId ? $groupMap[$groupPanelId] : undefined);
  let admin = $derived(isAdmin(group, me));
  let owner = $derived(isOwner(group, me));
  let isChannel = $derived(group?.kind === "channel");
  /** Basic groups: private, ≤ 200 members, no server tag / slow mode / log. */
  let basic = $derived(group?.kind === "group");
  let media = $derived(group ? $groupMedia[group.id] ?? {} : {});
  let noun = $derived(isChannel ? "channel" : "group");
  let busy = $state(false);

  // Text drafts on the manage page.
  let dTitle = $state(""), dDesc = $state(""), dLink = $state("");
  let dRules = $state(""), dWelcome = $state(""), dAccent = $state("");
  let lastId = "";
  $effect(() => {
    const g = group;
    if (!$groupPanelId) { lastId = ""; page = "info"; stack = []; return; }
    if (g && g.id !== lastId) {
      lastId = g.id;
      page = "info"; stack = [];
      resetDrafts(g);
    }
  });
  function resetDrafts(g: Group) {
    dTitle = g.title; dDesc = g.description; dLink = g.settings.public_link;
    dRules = g.rules ?? ""; dWelcome = g.welcome ?? ""; dAccent = g.accent ?? "";
    myTag = memberOf(g, me)?.tag ?? "";
  }

  function go(p: Page) { stack = [...stack, page]; page = p; }
  function back() { page = stack.pop() ?? "info"; stack = [...stack]; }

  // ---- saving ----------------------------------------------------------
  function clone(g: Group): Group { return JSON.parse(JSON.stringify(g)) as Group; }

  /** Apply a change to the latest copy, log it, publish it. */
  async function commit(mut: (g: Group) => void, log?: string) {
    if (!group) return;
    const next = clone(group);
    mut(next);
    if (log) next.log = [...(next.log ?? []), { at: new Date().toLocaleString(), by: me, text: log }].slice(-200);
    busy = true;
    try {
      await api.updateGroup(next);
      await refreshGroups();
    } catch (e) {
      showToast(`Couldn't save: ${e}`, 5000);
    } finally {
      busy = false;
    }
  }
  const S = (g: Group) => g.settings as unknown as Record<string, unknown>;

  async function saveInfo() {
    const t = dTitle.trim();
    if (!t) { showToast(tr("Name can't be empty")); return; }
    const changes: string[] = [];
    if (group && t !== group.title) changes.push(`renamed to “${t}”`);
    if (group && dDesc.trim() !== group.description) changes.push("changed the description");
    if (group && dRules.trim() !== (group.rules ?? "")) changes.push("updated the rules");
    if (group && dWelcome.trim() !== (group.welcome ?? "")) changes.push("changed the welcome message");
    if (group && dAccent !== (group.accent ?? "")) changes.push("changed the accent colour");
    await commit((g) => {
      g.title = t; g.description = dDesc.trim();
      g.rules = dRules.trim() || undefined; g.welcome = dWelcome.trim() || undefined; g.accent = dAccent || undefined;
    }, changes.join(", ") || undefined);
    showToast(tr("Saved"));
  }

  // ---- photo & banner --------------------------------------------------
  let uploading = $state<"" | "photo" | "banner">("");
  async function pickGroupImage(kind: "photo" | "banner") {
    if (!group) return;
    try {
      const result = await openFileDialog({
        multiple: false, directory: false, title: tr(kind === "photo" ? "Pick a photo" : "Pick a banner"),
        filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }],
      });
      const path = Array.isArray(result) ? (result[0] ?? null) : result;
      if (!path) return;
      let file = path;
      if (!path.toLowerCase().endsWith(".gif")) {
        // Animated GIFs go as they are; stills are cropped and re-encoded.
        const raw = await api.readUserImage(path);
        const src = URL.createObjectURL(new Blob([new Uint8Array(raw)]));
        const blob = await cropImage(src, kind === "photo"
          ? { aspect: 1, round: true, outWidth: 512, title: tr("Position the photo") }
          : { aspect: 3, outWidth: 1500, title: tr("Position the banner") });
        URL.revokeObjectURL(src);
        if (!blob) return;
        file = await api.saveTempFile(`${group.id}-${kind}.webp`, Array.from(new Uint8Array(await blob.arrayBuffer())));
      }
      uploading = kind;
      const m = await api.uploadAttachment(group.id, file);
      await commit((g) => { g[kind] = m; }, kind === "photo" ? "changed the photo" : "changed the banner");
      showToast(tr(kind === "photo" ? "Photo updated" : "Banner updated"));
    } catch (e) {
      showToast(`${tr("Couldn't upload")}: ${e}`, 5000);
    } finally {
      uploading = "";
    }
  }
  async function removeGroupImage(kind: "photo" | "banner") {
    await commit((g) => { g[kind] = null; }, kind === "photo" ? "removed the photo" : "removed the banner");
  }

  // ---- group ⇄ supergroup ----------------------------------------------
  const BASIC_LIMIT = 200;
  async function setKind(k: "group" | "supergroup") {
    if (!group || group.kind === k || !owner) return;
    if (k === "group") {
      if (group.members.length > BASIC_LIMIT) { showToast(tr("A group can have at most 200 members — this one is bigger"), 5000); return; }
      const ok = await askConfirm({
        title: tr("Turn into a regular group?"),
        text: tr("Regular groups are private and small: the public link, server tag, slow mode, hidden member list and admin log are switched off. Settings are kept and come back if you upgrade again."),
        ok: tr("Convert"),
      });
      if (!ok) return;
      await commit((g) => { g.kind = "group"; g.settings.is_public = false; }, "converted the supergroup to a group");
    } else {
      await commit((g) => {
        g.kind = "supergroup";
        if (!(g.rooms ?? []).length) { S(g).rooms_enabled = true; g.rooms = DEFAULT_ROOMS.map((r) => ({ ...r })); }
      }, "upgraded the group to a supergroup");
      showToast(tr("Now a supergroup: rooms like Discord channels, public link, server tag, slow mode and admin log"), 5000);
    }
  }

  // ---- rooms (Discord-style channels) -----------------------------------
  let roomList = $derived<Room[]>(group?.rooms ?? []);
  const slugRoom = (n: string) => n.trim().toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "-").replace(/^-|-$/g, "").slice(0, 24) || "room";
  async function setRoomsEnabled(on: boolean) {
    await commit((g) => {
      S(g).rooms_enabled = on;
      if (on && !(g.rooms ?? []).length) g.rooms = DEFAULT_ROOMS.map((r) => ({ ...r }));
    }, on ? "turned rooms on" : "turned rooms off");
  }
  async function saveRooms(next: Room[], log?: string) {
    if (!next.some((r) => r.kind === "text")) { showToast(tr("Keep at least one text room")); return; }
    await commit((g) => { g.rooms = next; }, log);
  }
  async function addRoom(kind: "text" | "voice") {
    const base = kind === "voice" ? tr("Voice") : tr("new-room");
    let id = slugRoom(base), n = 2;
    while (roomList.some((r) => r.id === id)) id = `${slugRoom(base)}-${n++}`;
    await saveRooms([...roomList, { id, name: kind === "voice" ? base : id, kind }], `added room ${id}`);
  }
  function patchRoom(id: string, p: Partial<Room>) { return roomList.map((r) => (r.id === id ? { ...r, ...p } : r)); }
  function moveRoom(i: number, d: number) {
    const a = [...roomList]; const j = i + d;
    if (j < 0 || j >= a.length) return;
    [a[i], a[j]] = [a[j], a[i]];
    void saveRooms(a);
  }

  // ---- server stats (fun) ------------------------------------------------
  let loadedMsgs = $state<{ gid: string; list: ChatMessage[] }>({ gid: "", list: [] });
  $effect(() => {
    const gid = group?.id;
    if (!gid || gid === $activeChat?.id || loadedMsgs.gid === gid) return;
    void api.getMessages(gid).then((list) => { loadedMsgs = { gid, list }; }).catch(() => {});
  });
  let stats = $derived.by(() => {
    if (!group) return null;
    const msgs = group.id === $activeChat?.id ? $activeMessages : loadedMsgs.gid === group.id ? loadedMsgs.list : [];
    const week = Date.now() - 7 * 864e5;
    const count: Record<string, number> = {};
    let today = 0;
    const d0 = new Date(); d0.setHours(0, 0, 0, 0);
    for (const m of msgs) {
      const at = Date.parse(m.time);
      if (!Number.isFinite(at)) continue;
      if (at >= d0.getTime()) today++;
      if (at >= week && m.from) count[m.from] = (count[m.from] ?? 0) + 1;
    }
    const top = Object.entries(count).sort((a, b) => b[1] - a[1]).slice(0, 5);
    const created = Date.parse(group.created_at);
    const ageDays = Number.isFinite(created) ? Math.max(0, Math.floor((Date.now() - created) / 864e5)) : null;
    return { top, today, total: msgs.length, ageDays, max: top[0]?.[1] ?? 1 };
  });

  // ---- my own stuff ----------------------------------------------------
  let myTag = $state("");
  let canSetOwnTag = $derived(!!group && (admin || (group.settings.edit_own_tags ?? "all") !== "admins"));
  async function saveMyTag() {
    if (!group) return;
    try { await api.groupSetMyTag(group.id, myTag); await refreshGroups(); showToast(myTag ? "Tag set" : "Tag removed"); }
    catch (e) { showToast(`${e}`); }
  }
  let wearing = $derived(!!group && $profile.clan?.gid === group.id);
  async function wearClan(on: boolean) {
    if (!group?.clan) return;
    const clan = on ? { ...group.clan, gid: group.id, title: group.title, owner: group.owner } : undefined;
    updateProfile({ clan });
    try { await publishMyProfile(); } catch { /* offline: saved locally */ }
    showToast(on ? `You now wear ${group.clan.text}` : "Server tag removed from your profile");
  }

  async function leave() {
    if (!group) return;
    busy = true;
    try {
      await api.leaveGroup(group.id);
      await refreshGroups();
      closeGroupPanel();
      backToList();
      showToast(`Left the ${noun}`);
    } catch (e) {
      showToast(`Could not leave: ${e}`, 5000);
    } finally {
      busy = false;
    }
  }

  async function copyLink() {
    if (!group) return;
    try { await navigator.clipboard.writeText(shareLink(group)); showToast(tr("Invite link copied — paste it into search to join")); }
    catch { showToast(tr("Copy blocked")); }
  }

  // ---- members ---------------------------------------------------------
  let addUser = $state("");
  let memberFilter = $state("");
  let openMember = $state("");
  async function addMember() {
    const u = addUser.trim().toLowerCase().replace(/^@/, "");
    if (!u || !group) return;
    if (group.members.some((m) => m.username === u)) { showToast(tr("Already here")); return; }
    if (basic && group.members.length >= BASIC_LIMIT) { showToast(tr("A group can have at most 200 members — upgrade it to a supergroup in Edit → Group type"), 6000); return; }
    if (get(peerPresence)[u]?.ni) {
      try { await navigator.clipboard.writeText(shareLink(group)); } catch { /* ignore */ }
      showToast(`@${u} only lets certain people add them to groups — the invite link is copied, send it to them`, 7000);
      return;
    }
    addUser = "";
    await commit((g) => {
      g.members.push({ username: u, role: "member" });
      g.banned = (g.banned ?? []).filter((b) => b !== u);
      g.requests = (g.requests ?? []).filter((r) => r.username !== u);
    }, `added @${u}`);
  }
  async function removeMember(u: string, ban: boolean) {
    await commit((g) => {
      g.members = g.members.filter((m) => m.username !== u);
      if (ban) g.banned = [...new Set([...(g.banned ?? []), u])];
    }, `${ban ? "banned" : "removed"} @${u}`);
    openMember = "";
  }
  async function setMemberField(u: string, patch: Partial<GroupMember>, log: string) {
    await commit((g) => { g.members = g.members.map((m) => (m.username === u ? { ...m, ...patch } : m)); }, log);
  }
  async function promote(u: string) {
    await setMemberField(u, { role: "admin", rights: defaultRights() }, `promoted @${u} to admin`);
    editUser = u; go("admin-edit");
  }
  function defaultRights(): AdminRights {
    return { change_info: true, post_messages: true, edit_messages: true, delete_messages: true, ban_users: true, invite_users: true, pin_messages: true, manage_tags: true, add_admins: false, anonymous: false };
  }

  // Tag editing for others (admins with manage_tags).
  let tagDraft = $state("");

  // ---- admin rights page ------------------------------------------------
  const RIGHT_LABELS: Record<keyof AdminRights, string> = {
    change_info: "Change info",
    post_messages: "Post messages",
    edit_messages: "Edit messages of others",
    delete_messages: "Delete messages",
    ban_users: "Ban users",
    invite_users: "Invite users via link",
    pin_messages: "Pin messages",
    manage_tags: "Edit member tags",
    add_admins: "Add new admins",
    anonymous: "Remain anonymous",
  };
  let editMember = $derived(group && editUser ? memberOf(group, editUser) : undefined);
  let titleDraft = $state("");
  $effect(() => { if (page === "admin-edit" && editMember) titleDraft = editMember.title ?? ""; });
  function rightsVisible(r: keyof AdminRights): boolean {
    if (isChannel) return r !== "manage_tags" && r !== "anonymous" && r !== "pin_messages";
    return r !== "post_messages" && r !== "edit_messages";
  }
  async function toggleRight(r: keyof AdminRights) {
    if (!editMember) return;
    const rights = { ...defaultRights(), ...(editMember.rights ?? {}) };
    rights[r] = !rights[r];
    await setMemberField(editMember.username, { rights }, `changed @${editMember.username}'s admin rights`);
  }

  // ---- permissions -----------------------------------------------------
  const PERM_ROWS: { key: string; label: string }[] = [
    { key: "send_messages", label: "Send messages" },
    { key: "send_media", label: "Send media" },
    { key: "send_photos", label: "Photos" },
    { key: "send_videos", label: "Videos" },
    { key: "send_files", label: "Files" },
    { key: "send_music", label: "Music" },
    { key: "send_voice", label: "Voice messages" },
    { key: "send_video_msgs", label: "Video messages" },
    { key: "send_stickers", label: "Stickers & GIFs" },
    { key: "send_polls", label: "Polls" },
    { key: "embed_links", label: "Embed links" },
    { key: "add_members", label: "Add members" },
    { key: "pin_messages", label: "Pin messages" },
    { key: "change_info", label: "Change chat info" },
    { key: "edit_own_tags", label: "Edit own tags" },
  ];
  const isOn = (g: Group, key: string) => (S(g)[key] ?? "all") !== "admins";
  async function togglePerm(key: string, label: string) {
    if (!group) return;
    const on = isOn(group, key);
    await commit((g) => { S(g)[key] = (on ? "admins" : "all") as Perm; }, `${on ? "restricted" : "allowed"} “${label}” for members`);
  }
  const SLOW = [0, 10, 30, 60, 300, 900, 3600];
  const slowLabel = (s: number) => (s === 0 ? "Off" : s < 60 ? `${s}s` : s < 3600 ? `${s / 60}m` : "1h");

  // ---- reactions -------------------------------------------------------
  let reactionChoices = $derived($reactionPack.items.length ? $reactionPack.items.map((i) => i.token) : quickReactions());
  async function setReactionMode(mode: "all" | "some" | "none") {
    await commit((g) => {
      S(g).reactions = mode;
      if (mode === "some" && !(g.settings.allowed_reactions ?? []).length) S(g).allowed_reactions = quickReactions($reactionPack.items);
    }, `set reactions to ${mode}`);
  }
  async function toggleAllowedReaction(r: string) {
    await commit((g) => {
      const cur = new Set(g.settings.allowed_reactions ?? []);
      if (cur.has(r)) cur.delete(r); else cur.add(r);
      S(g).allowed_reactions = [...cur];
    });
  }

  // ---- settings toggles -------------------------------------------------
  async function toggleSetting(key: string, label: string) {
    if (!group) return;
    const v = !S(group)[key];
    await commit((g) => { S(g)[key] = v; }, `${v ? "enabled" : "disabled"} “${label}”`);
  }

  // ---- join requests ---------------------------------------------------
  async function approve(u: string, ok: boolean) {
    await commit((g) => {
      g.requests = (g.requests ?? []).filter((r) => r.username !== u);
      if (ok && !g.members.some((m) => m.username === u)) g.members.push({ username: u, role: "member" });
    }, ok ? `approved @${u}` : `declined @${u}'s request`);
  }

  // ---- server tag ------------------------------------------------------
  let clanText = $state(""), clanBadge = $state("shield"), clanColor = $state(CLAN_COLORS[0]);
  $effect(() => {
    if (page === "clan" && group) {
      clanText = group.clan?.text ?? group.title.replace(/[^A-Za-z0-9А-Яа-я]/g, "").slice(0, 4).toUpperCase();
      clanBadge = group.clan?.badge ?? "shield";
      clanColor = group.clan?.color ?? CLAN_COLORS[0];
    }
  });
  async function saveClan(remove = false) {
    const text = clanText.trim().slice(0, 4);
    if (!remove && !/^[\p{L}\p{N}!?#@$&*+\-_.]{1,4}$/u.test(text)) { showToast("1–4 letters, digits or !?#@$&*+-_."); return; }
    await commit((g) => { g.clan = remove ? null : { text, badge: clanBadge, color: clanColor }; }, remove ? "removed the server tag" : `set server tag ${text}`);
    if (!remove) showToast(tr("Server tag saved — members can wear it on their profile"));
  }

  const filteredMembers = $derived.by(() => {
    if (!group) return [];
    const f = memberFilter.trim().toLowerCase();
    const rank = (m: GroupMember) => (m.role === "owner" ? 0 : m.role === "admin" ? 1 : 2);
    return [...group.members]
      .filter((m) => !f || m.username.includes(f) || (m.tag ?? "").toLowerCase().includes(f))
      .sort((a, b) => rank(a) - rank(b) || a.username.localeCompare(b.username));
  });
  const admins = $derived(group ? group.members.filter((m) => m.role !== "member") : []);
  const showMembers = $derived(!!group && (admin || basic || !group.settings.hide_members));

  const PAGE_TITLE: Record<Page, string> = {
    info: "", manage: "Edit", type: "Type & link", perms: "Permissions", reactions: "Reactions",
    admins: "Administrators", "admin-edit": "Admin rights", members: "Members", banned: "Removed users",
    requests: "Join requests", clan: "Server tag", log: "Recent actions", rooms: "Rooms",
  };
  const I = {
    pencil: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/></svg>`,
    back: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>`,
  };
</script>

{#snippet row(label: string, value: string, onclick: () => void, color = "")}
  <button class="row" {onclick}>
    {#if color}<span class="tile" style:background={color}></span>{/if}
    <span class="rl">{$t(label)}</span>
    <span class="rv">{$t(value)}</span>
    <span class="chev">›</span>
  </button>
{/snippet}

{#snippet toggle(label: string, sub: string, on: boolean, onclick: () => void, disabled = false)}
  <button class="row" {onclick} {disabled}>
    <span class="rl">{$t(label)}{#if sub}<span class="rs">{$t(sub)}</span>{/if}</span>
    <span class="switch" class:on></span>
  </button>
{/snippet}

{#snippet memberAva(u: string, size = 34)}
  <span class="m-ava" style:width="{size}px" style:height="{size}px">{u.charAt(0).toUpperCase()}</span>
{/snippet}

<svelte:window onkeydown={(e) => {
  if (!$groupPanelId || e.key !== "Escape" || e.defaultPrevented) return;
  const tgt = e.target as HTMLElement;
  if (tgt?.closest?.(".pop")) return;
  e.preventDefault();
  if (page !== "info") back(); else closeGroupPanel();
}} />

{#if $groupPanelId && group}
  <div class="backdrop" role="button" tabindex="-1"
       onclick={(e) => { if ((e.target as HTMLElement).classList.contains('backdrop')) closeGroupPanel(); }}
       onkeydown={() => {}}>
    <div class="sheet" role="dialog" aria-modal="true" aria-label="{noun} info">
      <div class="bar">
        {#if page !== "info"}
          <button class="ib" onclick={back} aria-label={$t("Back")}>{@html I.back}</button>
          <span class="bar-t">{$t(PAGE_TITLE[page])}</span>
        {:else}
          <span class="bar-t">{$t(isChannel ? "Channel info" : "Group info")}</span>
        {/if}
        <span class="sp"></span>
        {#if page === "info" && admin}
          <button class="ib" onclick={() => { resetDrafts(group); go("manage"); }} title={$t("Manage")} aria-label={$t("Manage")}>{@html I.pencil}</button>
        {/if}
        <button class="ib x" onclick={closeGroupPanel} aria-label={$t("Close")}>×</button>
      </div>

      <div class="body">
      {#if page === "info"}
        {#if media.banner || group.accent}
          <div class="gbanner" style:background={media.banner ? `center / cover no-repeat url("${media.banner}")` : `linear-gradient(135deg, ${group.accent}, color-mix(in srgb, ${group.accent} 40%, #000))`}></div>
        {/if}
        <div class="hero" class:over={!!(media.banner || group.accent)}>
          {#if media.photo}
            <img class="ava img" src={media.photo} alt="" style:--ring={group.accent ?? "transparent"} />
          {:else}
            <span class="ava" style:background={group.gradient} style:--ring={group.accent ?? "transparent"}>{group.title.charAt(0).toUpperCase()}</span>
          {/if}
          <div class="hero-t">
            <h2>{group.title} {#if group.clan}<ClanTag tag={{ ...group.clan, gid: group.id, title: group.title, owner: group.owner }} size="md" />{/if}</h2>
            <div class="sub">{#if !isChannel}<span class="kind">{$t(basic ? "Group" : "Supergroup")}</span> · {/if}{$tn(group.members.length, isChannel ? "subscriber" : "member")}{$t(group.settings.is_public ? " · public" : " · private")}{group.settings.protected_content ? $t(" · protected") : ""}</div>
          </div>
        </div>
        {#if group.description}<p class="desc">{group.description}</p>{/if}

        {#if group.rules}
          <details class="card rules" style:--acc={group.accent ?? "var(--accent)"}>
            <summary class="ct">{$t("Rules")}</summary>
            <p class="rules-t">{group.rules}</p>
          </details>
        {/if}

        {#if stats && stats.total > 0 && !isChannel}
          <div class="card">
            <div class="ct">{$t("This week")}</div>
            <div class="stat-row">
              <div class="stat"><b>{stats.today}</b><span>{$t("messages today")}</span></div>
              <div class="stat"><b>{stats.total}</b><span>{$t("in history")}</span></div>
              {#if stats.ageDays !== null}<div class="stat"><b>{stats.ageDays}</b><span>{$t("days old")}</span></div>{/if}
            </div>
            {#each stats.top as [u, n], i (u)}
              <div class="leader">
                <span class="medal">{["🥇", "🥈", "🥉", "4", "5"][i]}</span>
                <span class="m-name">{u}</span>
                <span class="lbar"><span style:width="{(n / stats.max) * 100}%" style:background={group.accent ?? "var(--accent)"}></span></span>
                <span class="ln">{n}</span>
              </div>
            {/each}
          </div>
        {/if}

        <div class="card">
          <button class="row" onclick={copyLink}>
            <span class="rl">{$t("Invite link")}<span class="rs mono">{shareLink(group).slice(0, 46)}…</span></span>
            <span class="rv">{$t("Copy")}</span>
          </button>
          {@render toggle("Notifications", $mutedChats.has(group.id) ? "Muted — no sound for new messages" : "On", !$mutedChats.has(group.id), () => setMuted(group.id))}
        </div>

        {#if !isChannel}
          <div class="card">
            <div class="ct">{$t(isChannel ? "Your tag in this channel" : "Your tag in this group")}</div>
            {#if canSetOwnTag}
              <div class="inline">
                <input class="inp" maxlength="24" placeholder={$t("e.g. designer, 11-B, moderator")} bind:value={myTag}
                  onkeydown={(e) => { if (e.key === "Enter") saveMyTag(); }} />
                <button class="btn" onclick={saveMyTag}>{$t("Set")}</button>
              </div>
              <div class="note">{$t("Shown next to your name in this chat. Admin tags are coloured.")}</div>
            {:else}
              <div class="note">{memberOf(group, me)?.tag ? `Your tag: ${memberOf(group, me)?.tag}` : "Only admins can set member tags here."}</div>
            {/if}
          </div>
        {/if}

        {#if group.clan && !basic}
          <div class="card">
            <div class="ct">{$t("Server tag")}</div>
            <div class="inline">
              <ClanTag tag={{ ...group.clan, gid: group.id, title: group.title, owner: group.owner }} size="md" />
              <span class="sp"></span>
              {#if wearing}
                <button class="btn ghost" onclick={() => wearClan(false)}>{$t("Stop wearing")}</button>
              {:else}
                <button class="btn" onclick={() => wearClan(true)}>{$t("Wear on my profile")}</button>
              {/if}
            </div>
            <div class="note">{$t("Shows next to your name everywhere — anyone can click it to see this chat and ask to join.")}</div>
          </div>
        {/if}

        {#if showMembers}
          <div class="card">
            <div class="ct">{$t(isChannel ? "Subscribers" : "Members")}</div>
            {#each filteredMembers.slice(0, 50) as m (m.username)}
              {@const mt = tagOf(group, m.username)}
              <div class="mrow">
                {@render memberAva(m.username)}
                <span class="m-name">{m.username}{#if m.username === me}<em> {$t("(you)")}</em>{/if}
                  {#if mt?.tag}<span class="mtag" class:admin={mt.admin}>{mt.tag}</span>{/if}</span>
                {#if mt?.title}<span class="m-role" class:owner={m.role === "owner"}>{mt.title}</span>{/if}
              </div>
            {/each}
            {#if group.members.length > 50}<button class="row" onclick={() => go("members")}><span class="rl">All {group.members.length}</span><span class="chev">›</span></button>{/if}
          </div>
        {/if}

        <button class="danger-btn" onclick={leave} disabled={busy}>{owner ? `Delete and leave ${noun}` : `Leave ${noun}`}</button>

      {:else if page === "manage"}
        {#if can(group, me, "change_info")}
          <div class="card media-edit">
            <div class="me-banner" style:background={media.banner ? `center / cover no-repeat url("${media.banner}")` : (group.accent ? group.accent : "var(--surface-2, #2a2f3a)")}>
              <button class="me-btn" onclick={() => pickGroupImage("banner")} disabled={!!uploading}>{$t(uploading === "banner" ? "Uploading…" : media.banner ? "Change banner" : "Add banner")}</button>
              {#if group.banner}<button class="me-btn ghost" onclick={() => removeGroupImage("banner")} disabled={busy}>{$t("Remove")}</button>{/if}
            </div>
            <div class="me-photo">
              <button class="me-ava" onclick={() => pickGroupImage("photo")} disabled={!!uploading} title={$t("Change photo")}>
                {#if media.photo}<img src={media.photo} alt="" />{:else}<span style:background={group.gradient}>{group.title.charAt(0).toUpperCase()}</span>{/if}
                <i>{uploading === "photo" ? "…" : "📷"}</i>
              </button>
              <div class="me-actions">
                <button class="btn" onclick={() => pickGroupImage("photo")} disabled={!!uploading}>{$t(media.photo ? "Change photo" : "Set photo")}</button>
                {#if group.photo}<button class="btn ghost" onclick={() => removeGroupImage("photo")} disabled={busy}>{$t("Remove")}</button>{/if}
              </div>
            </div>
            <div class="note">{$t("PNG, JPG, WebP or an animated GIF up to 4 MB. Encrypted for members; the relay keeps it while it's being viewed.")}</div>
          </div>
        {/if}
        <div class="card">
          <label class="fld"><span>{$t("Name")}</span><input class="inp" bind:value={dTitle} maxlength="64" disabled={!can(group, me, "change_info")} /></label>
          <label class="fld"><span>{$t("Description")}</span><textarea class="inp" rows="3" bind:value={dDesc} maxlength="255" disabled={!can(group, me, "change_info")}></textarea></label>
          {#if !isChannel}
            <label class="fld"><span>{$t("Rules")}</span><textarea class="inp" rows="4" bind:value={dRules} maxlength="1500" placeholder={$t("1. Be nice\n2. No spam")} disabled={!can(group, me, "change_info")}></textarea></label>
            <label class="fld"><span>{$t("Welcome message")}</span><input class="inp" bind:value={dWelcome} maxlength="200" placeholder={$t("Shown to people who just joined")} disabled={!can(group, me, "change_info")} /></label>
          {/if}
          <div class="fld"><span>{$t("Accent colour")}</span>
            <div class="colors">
              <button class="ccell none" class:on={!dAccent} onclick={() => (dAccent = "")} aria-label={$t("None")} disabled={!can(group, me, "change_info")}>∅</button>
              {#each CLAN_COLORS as c (c)}<button class="ccell" class:on={dAccent === c} style:background={c} onclick={() => (dAccent = c)} aria-label={c} disabled={!can(group, me, "change_info")}></button>{/each}
              <input type="color" value={dAccent || "#3fb6a8"} oninput={(e) => (dAccent = (e.target as HTMLInputElement).value)} aria-label={$t("Custom colour")} disabled={!can(group, me, "change_info")} />
            </div>
          </div>
          {#if can(group, me, "change_info")}<button class="btn wide" onclick={saveInfo} disabled={busy}>{$t("Save")}</button>{/if}
        </div>
        {#if !isChannel && owner}
          <div class="card">
            <div class="ct">{$t("Chat type")}</div>
            <div class="seg">
              <button class:active={basic} onclick={() => setKind("group")} disabled={busy}>{$t("Group")}</button>
              <button class:active={!basic} onclick={() => setKind("supergroup")} disabled={busy}>{$t("Supergroup")}</button>
            </div>
            <div class="note">{$t(basic
              ? "Group: private, up to 200 members. Upgrade for a public link, server tag, slow mode, hidden members and the admin log."
              : "Supergroup: up to 200 000 members, public link, server tag, slow mode, hidden members and the admin log.")}</div>
          </div>
        {/if}
        <div class="card">
          {#if !basic}{@render row(isChannel ? "Channel type" : "Group type", group.settings.is_public ? "Public" : "Private", () => go("type"), "#007AFF")}{/if}
          {#if !isChannel}{@render row("Permissions", `${PERM_ROWS.filter((p) => isOn(group, p.key)).length}/${PERM_ROWS.length}`, () => go("perms"), "#34C759")}{/if}
          {@render row("Reactions", group.settings.reactions === "none" ? "Off" : group.settings.reactions === "some" ? `${group.settings.allowed_reactions?.length ?? 0}` : "All", () => go("reactions"), "#FF9500")}
          {@render row("Administrators", `${admins.length}`, () => go("admins"), "#5856D6")}
          {@render row(isChannel ? "Subscribers" : "Members", `${group.members.length}`, () => go("members"), "#32ADE6")}
          {#if !basic}{@render row("Join requests", `${group.requests?.length ?? 0}`, () => go("requests"), "#FF2D55")}{/if}
          {@render row("Removed users", `${group.banned?.length ?? 0}`, () => go("banned"), "#FF3B30")}
          {#if !basic && !isChannel}
            {@render row("Rooms", group.settings.rooms_enabled ? `${roomList.length}` : "Off", () => go("rooms"), "#00C7BE")}
          {/if}
          {#if !basic}
            {@render row("Server tag", group.clan?.text ?? "None", () => go("clan"), "#AF52DE")}
            {@render row("Recent actions", `${group.log?.length ?? 0}`, () => go("log"), "#8E8E93")}
          {/if}
        </div>
        <div class="card">
          {#if !isChannel}
            {@render toggle("Chat history for new members", group.settings.history_visible ? "Visible" : "Hidden", group.settings.history_visible, () => toggleSetting("history_visible", "history for new members"))}
          {/if}
          {#if !basic}{@render toggle("Approve new members", "Admins must approve anyone who joins by link or server tag", !!group.settings.join_requests, () => toggleSetting("join_requests", "approve new members"))}{/if}
          {@render toggle("Restrict saving content", "No forwarding, copying or saving media", !!group.settings.protected_content, () => toggleSetting("protected_content", "restrict saving content"))}
          {#if isChannel}
            {@render toggle("Sign messages", "Add the admin's name to posts", group.settings.sign_messages, () => toggleSetting("sign_messages", "sign messages"))}
            {@render toggle("Show authors' profiles", "Signatures link to the author", !!group.settings.show_authors, () => toggleSetting("show_authors", "show authors' profiles"))}
          {:else if !basic}
            {@render toggle("Hide members", "Only admins see the member list", !!group.settings.hide_members, () => toggleSetting("hide_members", "hide members"))}
          {/if}
        </div>

      {:else if page === "type"}
        <div class="card">
          <div class="seg">
            <button class:active={!group.settings.is_public} onclick={() => !group.settings.is_public || commit((g) => { g.settings.is_public = false; }, `made the ${noun} private`)}>{$t("Private")}</button>
            <button class:active={group.settings.is_public} onclick={() => group.settings.is_public || commit((g) => { g.settings.is_public = true; }, `made the ${noun} public`)}>{$t("Public")}</button>
          </div>
          <div class="note">{group.settings.is_public
            ? `Anyone with the link or the server tag can join${$t(group.settings.join_requests ? " after an admin approves" : " instantly")}.`
            : `People can only join if an admin adds them or approves their request.`}</div>
        </div>
        <div class="card">
          <label class="fld"><span>{$t("Public name")}</span><input class="inp" bind:value={dLink} maxlength="32" placeholder={$t("mygroup")} /></label>
          <button class="btn wide" onclick={() => commit((g) => { g.settings.public_link = dLink.trim().toLowerCase(); }, "changed the public name")}>{$t("Save")}</button>
        </div>
        <div class="card">
          <button class="row" onclick={copyLink}><span class="rl">{$t("Invite link")}<span class="rs mono">{shareLink(group)}</span></span><span class="rv">{$t("Copy")}</span></button>
          <div class="note">{$t("Paste the link into Aquamarine's search to send a join request.")}</div>
        </div>

      {:else if page === "perms"}
        <div class="card">
          <div class="ct">{$t("What can members of this group do?")}</div>
          {#each PERM_ROWS as p (p.key)}
            {@render toggle(p.label, "", isOn(group, p.key), () => togglePerm(p.key, p.label), busy)}
          {/each}
        </div>
        <div class="card" class:hidden={basic}>
          <div class="ct">{$t("Slow mode")}</div>
          <div class="seg">
            {#each SLOW as v}
              <button class:active={group.settings.slow_mode === v} onclick={() => commit((g) => { g.settings.slow_mode = v; }, `set slow mode to ${slowLabel(v)}`)}>{slowLabel(v)}</button>
            {/each}
          </div>
          <div class="note">{$t("Members can send one message per interval.")}</div>
        </div>

      {:else if page === "reactions"}
        <div class="card">
          <div class="seg">
            <button class:active={(group.settings.reactions ?? "all") === "all"} onclick={() => setReactionMode("all")}>{$t("All reactions")}</button>
            <button class:active={group.settings.reactions === "some"} onclick={() => setReactionMode("some")}>{$t("Some")}</button>
            <button class:active={group.settings.reactions === "none"} onclick={() => setReactionMode("none")}>{$t("None")}</button>
          </div>
        </div>
        {#if group.settings.reactions === "some"}
          <div class="card">
            <div class="ct">{$t("Allowed reactions")}</div>
            <div class="rgrid">
              {#each reactionChoices as r (r)}
                {@const ru = reactionUrl(r)}
                <button class="rcell" class:on={(group.settings.allowed_reactions ?? []).includes(r)} onclick={() => toggleAllowedReaction(r)} title={parseReaction(r).emoji}>
                  {#if ru}<AnimatedMedia src={ru.url} mime={ru.mime} play="hover" alt={parseReaction(r).emoji} />{:else}{r}{/if}
                </button>
              {/each}
            </div>
          </div>
        {/if}

      {:else if page === "admins"}
        <div class="card">
          {#each admins as m (m.username)}
            <button class="mrow click" onclick={() => { if (m.role !== "owner" && (owner || can(group, me, "add_admins"))) { editUser = m.username; go("admin-edit"); } }}>
              {@render memberAva(m.username)}
              <span class="m-name">{m.username}</span>
              <span class="m-role" class:owner={m.role === "owner"}>{m.title || m.role}</span>
            </button>
          {/each}
        </div>
        <div class="note pad">{$t("To add an admin, open Members and pick someone.")}</div>

      {:else if page === "admin-edit" && editMember}
        <div class="card">
          <div class="mrow">{@render memberAva(editMember.username, 44)}<span class="m-name big">{editMember.username}</span></div>
          <label class="fld"><span>{$t("Custom title")}</span>
            <div class="inline"><input class="inp" maxlength="16" placeholder={$t(editMember.role === "owner" ? "owner" : "admin")} bind:value={titleDraft} />
              <button class="btn" onclick={() => setMemberField(editMember!.username, { title: titleDraft.trim() }, `set @${editMember!.username}'s title`)}>{$t("Set")}</button></div>
          </label>
        </div>
        <div class="card">
          <div class="ct">{$t("What can this admin do?")}</div>
          {#each ALL_RIGHTS.filter(rightsVisible) as r (r)}
            {@render toggle(RIGHT_LABELS[r], "", can(group, editMember.username, r), () => toggleRight(r), busy)}
          {/each}
        </div>
        <button class="danger-btn" onclick={async () => { await setMemberField(editMember!.username, { role: "member", rights: undefined, title: "" }, `dismissed @${editMember!.username}`); back(); }}>{$t("Dismiss admin")}</button>

      {:else if page === "members"}
        <div class="card">
          {#if owner || can(group, me, "invite_users") || group.settings.add_members === "all"}
            <div class="inline"><input class="inp" placeholder={$t("Add by username")} bind:value={addUser} onkeydown={(e) => e.key === "Enter" && addMember()} />
              <button class="btn" onclick={addMember}>{$t("Add")}</button></div>
          {/if}
          <input class="inp search" placeholder={$t("Search members")} bind:value={memberFilter} />
        </div>
        <div class="card">
          {#each filteredMembers as m (m.username)}
            {@const mt = tagOf(group, m.username)}
            <button class="mrow click" onclick={() => { openMember = openMember === m.username ? "" : m.username; tagDraft = m.tag ?? ""; }}>
              {@render memberAva(m.username)}
              <span class="m-name">{m.username}{#if m.username === me}<em> {$t("(you)")}</em>{/if}
                {#if mt?.tag}<span class="mtag" class:admin={mt.admin}>{mt.tag}</span>{/if}
                {#if m.muted}<span class="mtag muted">{$t("restricted")}</span>{/if}</span>
              {#if mt?.title}<span class="m-role" class:owner={m.role === "owner"}>{mt.title}</span>{/if}
            </button>
            {#if openMember === m.username && m.username !== me && m.role !== "owner"}
              <div class="macts">
                {#if can(group, me, "manage_tags")}
                  <div class="inline"><input class="inp" maxlength="24" placeholder={$t("Member tag")} bind:value={tagDraft} />
                    <button class="btn" onclick={() => setMemberField(m.username, { tag: tagDraft.trim() }, `set @${m.username}'s tag`)}>{$t("Set tag")}</button></div>
                {/if}
                <div class="abtns">
                  {#if m.role === "member" && (owner || can(group, me, "add_admins"))}<button class="btn" onclick={() => promote(m.username)}>{$t("Make admin")}</button>{/if}
                  {#if m.role === "admin" && (owner || can(group, me, "add_admins"))}<button class="btn" onclick={() => { editUser = m.username; go("admin-edit"); }}>{$t("Admin rights")}</button>{/if}
                  {#if can(group, me, "ban_users") && m.role === "member"}
                    <button class="btn ghost" onclick={() => setMemberField(m.username, { muted: !m.muted }, `${$t(m.muted ? "unrestricted" : "restricted")} @${m.username}`)}>{$t(m.muted ? "Allow sending" : "Restrict")}</button>
                    <button class="btn ghost" onclick={() => removeMember(m.username, false)}>{$t("Remove")}</button>
                    <button class="btn danger" onclick={() => removeMember(m.username, true)}>{$t("Ban")}</button>
                  {/if}
                </div>
              </div>
            {/if}
          {/each}
        </div>

      {:else if page === "banned"}
        <div class="card">
          {#each group.banned ?? [] as u (u)}
            <div class="mrow">{@render memberAva(u)}<span class="m-name">{u}</span>
              {#if can(group, me, "ban_users")}<button class="btn ghost" onclick={() => commit((g) => { g.banned = (g.banned ?? []).filter((b) => b !== u); }, `unbanned @${u}`)}>{$t("Unban")}</button>{/if}</div>
          {:else}
            <div class="note">{$t("Nobody has been removed. Banned users can't rejoin or send requests.")}</div>
          {/each}
        </div>

      {:else if page === "requests"}
        <div class="card">
          {#each group.requests ?? [] as r (r.username)}
            <div class="mrow">{@render memberAva(r.username)}
              <span class="m-name">{r.username}<span class="rs">{r.at}{r.note ? ` · ${r.note}` : ""}</span></span>
              <button class="btn" onclick={() => approve(r.username, true)}>{$t("Add")}</button>
              <button class="btn ghost" onclick={() => approve(r.username, false)}>{$t("Dismiss")}</button>
            </div>
          {:else}
            <div class="note">{$t("No pending requests. People ask to join via your invite link or your server tag.")}</div>
          {/each}
        </div>

      {:else if page === "clan"}
        <div class="card">
          <div class="preview"><ClanTag tag={{ text: clanText || "TAG", badge: clanBadge, color: clanColor }} size="md" /> <span class="pv-name">{me}</span></div>
          <label class="fld"><span>{$t("Tag (up to 4 characters)")}</span><input class="inp" maxlength="4" bind:value={clanText} /></label>
          <div class="ct">{$t("Badge")}</div>
          <div class="badges">
            {#each Object.keys(CLAN_BADGES) as b (b)}
              <button class="bcell" class:on={clanBadge === b} style:color={clanColor} onclick={() => (clanBadge = b)} title={b}>{@html badgeSvg(b)}</button>
            {/each}
          </div>
          <div class="ct">{$t("Colour")}</div>
          <div class="colors">
            {#each CLAN_COLORS as c (c)}<button class="ccell" class:on={clanColor === c} style:background={c} onclick={() => (clanColor = c)} aria-label={c}></button>{/each}
            <input type="color" bind:value={clanColor} aria-label={$t("Custom colour")} />
          </div>
          <div class="abtns">
            <button class="btn" onclick={() => saveClan()}>{$t("Save tag")}</button>
            {#if group.clan}<button class="btn ghost" onclick={() => saveClan(true)}>{$t("Remove")}</button>{/if}
          </div>
          <div class="note">{$t("Like Discord server tags: members can wear it next to their name everywhere, and anyone can click it to ask to join.")}</div>
        </div>

      {:else if page === "rooms"}
        <div class="card">
          {@render toggle("Rooms", "Split the chat into channels like a Discord server: text rooms, announcement rooms, voice rooms", !!group.settings.rooms_enabled, () => setRoomsEnabled(!group.settings.rooms_enabled), busy || !can(group, me, "change_info"))}
        </div>
        {#if group.settings.rooms_enabled}
          <div class="card">
            {#each roomList as r, i (r.id)}
              <div class="room-row">
                <input class="inp re" value={r.emoji ?? ""} maxlength="2" placeholder={r.kind === "voice" ? "🔊" : "#"}
                  onchange={(e) => saveRooms(patchRoom(r.id, { emoji: (e.target as HTMLInputElement).value.trim() || undefined }))} />
                <input class="inp rn" value={r.name} maxlength="32"
                  onchange={(e) => { const v = (e.target as HTMLInputElement).value.trim(); if (v) void saveRooms(patchRoom(r.id, { name: r.kind === "text" ? slugRoom(v) : v }), `renamed room to ${v}`); }} />
                {#if r.kind === "text"}
                  <button class="rr-btn" class:on={r.readonly} title={$t("Only admins write here")} onclick={() => saveRooms(patchRoom(r.id, { readonly: !r.readonly }))}>🔒</button>
                {:else}
                  <span class="rr-kind">{$t("voice")}</span>
                {/if}
                <button class="rr-btn" onclick={() => moveRoom(i, -1)} disabled={i === 0} aria-label={$t("Up")}>↑</button>
                <button class="rr-btn" onclick={() => moveRoom(i, 1)} disabled={i === roomList.length - 1} aria-label={$t("Down")}>↓</button>
                <button class="rr-btn danger" onclick={() => saveRooms(roomList.filter((x) => x.id !== r.id), `deleted room ${r.name}`)} aria-label={$t("Delete")}>×</button>
              </div>
            {/each}
            <div class="abtns">
              <button class="btn" onclick={() => addRoom("text")}># {$t("Text room")}</button>
              <button class="btn ghost" onclick={() => addRoom("voice")}>🔊 {$t("Voice room")}</button>
            </div>
            <div class="note">{$t("🔒 — announcement room: only admins write. Messages of a deleted room move to the first room. Voice rooms join the group's voice chat.")}</div>
          </div>
        {/if}

      {:else if page === "log"}
        <div class="card">
          {#each [...(group.log ?? [])].reverse() as l, i (i)}
            <div class="log"><b>@{l.by}</b> {l.text}<span class="rs">{l.at}</span></div>
          {:else}
            <div class="note">{$t("No admin actions yet.")}</div>
          {/each}
        </div>
      {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; background: rgba(6,7,12,0.6); backdrop-filter: blur(4px);
              display: grid; place-items: center; z-index: 130; padding: 20px; }
  .sheet { width: min(500px, 95vw); height: min(720px, 90vh); display: flex; flex-direction: column;
           background: var(--bg-shell); border: 1px solid var(--border-strong); border-radius: 18px;
           box-shadow: 0 40px 90px -24px rgba(0,0,0,0.7); overflow: hidden;  animation: aq-pop .3s var(--ease-spring, ease-out); }
  .bar { display: flex; align-items: center; gap: 6px; padding: 10px 12px; border-bottom: 1px solid var(--border); }
  .bar-t { font-weight: 800; font-size: 15px; color: var(--fg); }
  .sp { flex: 1; }
  .ib { width: 32px; height: 32px; border-radius: 8px; border: none; background: transparent; color: var(--fg-muted);
        display: grid; place-items: center; cursor: pointer; font-size: 20px; }
  .ib :global(svg) { width: 17px; height: 17px; }
  .ib:hover { background: var(--bg-hover); color: var(--fg); }
  .body { flex: 1; overflow-y: auto; padding: 12px 14px 18px; }

  .hero { display: flex; gap: 14px; align-items: center; padding: 6px 4px 10px; }
  .ava { width: 64px; height: 64px; border-radius: 50%; display: grid; place-items: center; flex-shrink: 0;
         color: #06070C; font-weight: 800; font-size: 26px; box-shadow: 0 0 0 3px var(--ring, transparent); }
  .ava.img { object-fit: cover; background: var(--bg-surface); }
  .gbanner { height: 120px; margin: -12px -14px 0; animation: gb-in .45s cubic-bezier(.2,.8,.2,1); }
  .hero.over { margin-top: -34px; position: relative; }
  .hero.over .ava { width: 76px; height: 76px; box-shadow: 0 0 0 4px var(--bg-shell), 0 0 0 6px var(--ring, transparent); }
  .hero.over .hero-t { padding-top: 30px; }
  .kind { font-weight: 700; color: var(--fg); }
  @keyframes gb-in { from { opacity: 0; transform: scale(1.04); } to { opacity: 1; transform: none; } }
  .rules summary { cursor: pointer; list-style: none; color: var(--acc); }
  .rules summary::before { content: "📜 "; }
  .rules summary::after { content: " ›"; display: inline-block; transition: transform .2s; }
  .rules[open] summary::after { transform: rotate(90deg); }
  .rules-t { margin: 0 0 10px; font-size: 13px; line-height: 1.5; white-space: pre-wrap; color: var(--fg); }
  .stat-row { display: flex; gap: 8px; margin-bottom: 8px; }
  .stat { flex: 1; background: var(--bg-input); border-radius: 10px; padding: 8px; display: flex; flex-direction: column; align-items: center; }
  .stat b { font-size: 18px; color: var(--fg); }
  .stat span { font-size: 11px; color: var(--fg-dim); }
  .leader { display: flex; align-items: center; gap: 8px; padding: 4px 0; font-size: 13px; }
  .medal { width: 22px; text-align: center; color: var(--fg-dim); font-weight: 700; }
  .lbar { flex: 1; height: 6px; border-radius: 3px; background: var(--bg-input); overflow: hidden; }
  .lbar span { display: block; height: 100%; border-radius: 3px; transition: width .6s cubic-bezier(.2,.8,.2,1); }
  .ln { min-width: 28px; text-align: right; color: var(--fg-muted); font-variant-numeric: tabular-nums; }
  .media-edit { padding: 0 0 6px; overflow: hidden; }
  .media-edit .note { padding: 6px 12px 4px; }
  .me-banner { height: 110px; display: flex; align-items: flex-end; justify-content: flex-end; gap: 6px; padding: 8px; }
  .me-btn { border: none; border-radius: 8px; padding: 6px 10px; font-size: 12px; font-weight: 700; cursor: pointer;
            background: rgba(0,0,0,.55); color: #fff; backdrop-filter: blur(6px); }
  .me-btn.ghost { background: rgba(0,0,0,.3); }
  .me-photo { display: flex; align-items: center; gap: 12px; padding: 0 12px; margin-top: -30px; }
  .me-ava { position: relative; width: 72px; height: 72px; border-radius: 50%; padding: 0; border: 4px solid var(--bg-surface);
            overflow: hidden; cursor: pointer; background: none; flex-shrink: 0; }
  .me-ava img, .me-ava span { width: 100%; height: 100%; object-fit: cover; display: grid; place-items: center;
            font-weight: 800; font-size: 26px; color: #06070C; }
  .me-ava i { position: absolute; inset: 0; display: grid; place-items: center; background: rgba(0,0,0,.45);
              opacity: 0; transition: opacity .2s; font-style: normal; font-size: 20px; }
  .me-ava:hover i { opacity: 1; }
  .me-actions { display: flex; gap: 6px; padding-top: 30px; }
  .ccell.none { background: var(--bg-input); color: var(--fg-dim); font-size: 13px; }
  .hidden { display: none; }
  .room-row { display: flex; gap: 6px; align-items: center; padding: 5px 0; border-bottom: 1px solid var(--border); }
  .room-row:last-of-type { border-bottom: none; }
  .room-row .re { flex: 0 0 44px; width: 44px; text-align: center; padding: 6px 4px; }
  .room-row .rn { flex: 1; }
  .rr-btn { width: 30px; height: 30px; border-radius: 8px; border: 1px solid var(--border); background: var(--bg-input); color: var(--fg-muted); cursor: pointer; flex-shrink: 0; }
  .rr-btn.on { border-color: var(--accent); background: var(--accent-soft); }
  .rr-btn.danger:hover { color: #ff6b6b; }
  .rr-btn:disabled { opacity: .35; }
  .rr-kind { font-size: 11px; color: var(--fg-dim); padding: 0 4px; }
  .hero h2 { margin: 0; font-size: 19px; font-weight: 800; display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .sub { font-size: 12.5px; color: var(--fg-muted); margin-top: 3px; }
  .desc { margin: 0 4px 10px; font-size: 13.5px; color: var(--fg); line-height: 1.45; white-space: pre-wrap; }

  .card { background: var(--bg-surface); border-radius: 14px; padding: 6px 12px; margin-bottom: 10px; }
  .ct { font-size: 12px; font-weight: 700; color: var(--accent); padding: 8px 0 6px; }
  .note { font-size: 11.5px; color: var(--fg-dim); padding: 6px 0 8px; line-height: 1.45; }
  .note.pad { padding: 0 6px; }
  .row { width: 100%; display: flex; align-items: center; gap: 10px; padding: 10px 0; background: transparent; border: none;
         color: var(--fg); text-align: left; cursor: pointer; font-size: 14px; border-bottom: 1px solid var(--border); }
  .row:last-child { border-bottom: none; }
  .row:disabled { opacity: 0.6; cursor: default; }
  .tile { width: 10px; height: 10px; border-radius: 3px; flex-shrink: 0; }
  .rl { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .rs { font-size: 11.5px; color: var(--fg-dim); font-weight: 400; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .rs.mono { font-family: var(--font-mono, monospace); }
  .rv { color: var(--fg-muted); font-size: 13px; }
  .chev { color: var(--fg-dim); font-size: 18px; }
  .switch { flex-shrink: 0; width: 38px; height: 22px; border-radius: 100px; background: var(--bg-input);
            border: 1px solid var(--border-strong); position: relative; }
  .switch::after { content: ''; position: absolute; top: 2px; left: 2px; width: 16px; height: 16px; border-radius: 50%;
                   background: var(--fg-dim); transition: transform 0.15s ease; }
  .switch.on { background: var(--accent-soft); border-color: var(--accent); }
  .switch.on::after { transform: translateX(16px); background: var(--accent); }

  .inline { display: flex; gap: 8px; align-items: center; padding: 6px 0; }
  .inp { flex: 1; min-width: 0; background: var(--bg-input); border: 1px solid var(--border); border-radius: 9px;
         color: var(--fg); font: inherit; font-size: 13.5px; padding: 8px 10px; outline: none; resize: vertical; box-sizing: border-box; width: 100%; }
  .inp:focus { border-color: var(--accent); }
  .inp.search { margin: 6px 0; }
  .fld { display: flex; flex-direction: column; gap: 5px; padding: 6px 0; }
  .fld > span { font-size: 11.5px; color: var(--fg-dim); font-weight: 600; }
  .btn { flex-shrink: 0; border: none; border-radius: 9px; padding: 7px 12px; font-weight: 700; font-size: 12.5px;
         cursor: pointer; background: var(--accent); color: #06070C; }
  .btn.wide { width: 100%; margin: 6px 0 8px; padding: 9px; }
  .btn.ghost { background: var(--bg-input); color: var(--fg); }
  .btn.danger { background: rgba(255, 77, 109, 0.16); color: #ff6b6b; }
  .btn:disabled { opacity: 0.6; }
  .danger-btn { width: 100%; border: none; border-radius: 12px; padding: 12px; margin-top: 4px; cursor: pointer;
                background: var(--bg-surface); color: #ff6b6b; font-weight: 700; font-size: 14px; }
  .seg { display: flex; gap: 4px; background: var(--bg-input); border-radius: 10px; padding: 3px; margin: 8px 0; flex-wrap: wrap; }
  .seg button { flex: 1; border: none; background: transparent; color: var(--fg-muted); font-weight: 600; font-size: 12.5px;
                padding: 7px 8px; border-radius: 8px; cursor: pointer; white-space: nowrap; }
  .seg button.active { background: var(--accent-soft); color: var(--accent); }

  .mrow { width: 100%; display: flex; align-items: center; gap: 10px; padding: 8px 0; background: transparent; border: none;
          color: inherit; text-align: left; font: inherit; }
  .mrow.click { cursor: pointer; }
  .mrow.click:hover .m-name { color: var(--accent); }
  .m-ava { border-radius: 50%; background: linear-gradient(135deg, var(--accent), var(--accent-2)); display: grid; place-items: center;
           font-weight: 800; font-size: 13px; color: #06070C; flex-shrink: 0; }
  .m-name { flex: 1; min-width: 0; font-size: 13.5px; color: var(--fg); display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .m-name.big { font-size: 16px; font-weight: 800; }
  .m-name em { color: var(--fg-dim); font-style: normal; }
  .m-name .rs { width: 100%; }
  .m-role { font-size: 11.5px; color: var(--fg-dim); }
  .m-role.owner { color: var(--accent); }
  .mtag { font-size: 10.5px; font-weight: 700; padding: 1px 6px; border-radius: 5px; background: var(--bg-input); color: var(--fg-muted); }
  .mtag.admin { background: color-mix(in srgb, var(--accent-2) 22%, transparent); color: var(--accent-2); }
  .mtag.muted { background: rgba(255, 77, 109, 0.14); color: #ff6b6b; }
  .macts { padding: 2px 0 10px 44px; }
  .abtns { display: flex; gap: 6px; flex-wrap: wrap; padding: 4px 0; }
  .log { font-size: 13px; color: var(--fg); padding: 8px 0; border-bottom: 1px solid var(--border); display: flex; flex-direction: column; }
  .log b { color: var(--accent); }

  .rgrid { display: grid; grid-template-columns: repeat(8, 1fr); gap: 4px; padding-bottom: 8px; }
  .rcell { aspect-ratio: 1; border-radius: 10px; border: 2px solid transparent; background: var(--bg-input); cursor: pointer;
           display: grid; place-items: center; padding: 3px; font-size: 20px; opacity: 0.45; }
  .rcell.on { border-color: var(--accent); opacity: 1; }
  .preview { display: flex; align-items: center; gap: 8px; padding: 10px 0; }
  .pv-name { font-weight: 700; color: var(--fg); }
  .badges { display: grid; grid-template-columns: repeat(8, 1fr); gap: 6px; }
  .bcell { aspect-ratio: 1; border-radius: 10px; border: 2px solid transparent; background: var(--bg-input); cursor: pointer;
           display: grid; place-items: center; }
  .bcell :global(svg) { width: 60%; height: 60%; }
  .bcell.on { border-color: currentColor; }
  .colors { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; padding-bottom: 6px; }
  .ccell { width: 26px; height: 26px; border-radius: 50%; border: 2px solid transparent; cursor: pointer; }
  .ccell.on { border-color: var(--fg); }
  .colors input[type=color] { width: 30px; height: 30px; border: none; background: none; cursor: pointer; }
</style>
