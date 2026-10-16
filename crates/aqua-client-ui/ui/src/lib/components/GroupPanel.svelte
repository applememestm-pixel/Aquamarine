<script module lang="ts">
  import { writable, get } from "svelte/store";
  import { peerPresence } from "../presence";
  export const groupPanelId = writable<string | null>(null);
  /** Open straight at a page (e.g. a room's settings from its context menu). */
  const groupPanelStart = writable<{ page: string; room?: string } | null>(null);
  export function openGroupPanel(gid: string, start?: { page: "room-edit" | "room-perms" | "rooms"; room?: string }) {
    groupPanelStart.set(start ?? null);
    groupPanelId.set(null);
    groupPanelId.set(gid);
  }
  export function closeGroupPanel() { groupPanelId.set(null); }
</script>

<script lang="ts">
  import ColorInput from "./ColorInput.svelte";
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
  import { streamSrc } from "../media";
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
  import { rolesOn, rolesOf, memberRoles, memberRoleIds, roleColor, hasPerm, canManageRole, outranks, rank, everyoneRole, PERM_GROUPS, ROLE_COLORS, ROOM_PERMS, type Role, type PermKey } from "../roles";
  import AnimatedMedia from "./AnimatedMedia.svelte";
  import ClanTag from "./ClanTag.svelte";
  import type { AdminRights, Group, GroupMember, Perm } from "../types";

  type Page = "room-edit" | "roles" | "role-edit" | "room-perms" | "rooms" | "info" | "manage" | "type" | "perms" | "reactions" | "admins" | "admin-edit" | "members" | "banned" | "requests" | "clan" | "log";
  let page = $state<Page>("info");
  let stack: Page[] = [];
  let editUser = $state("");

  let me = $derived($info?.username ?? "");
  let group = $derived($groupPanelId ? $groupMap[$groupPanelId] : undefined);
  let admin = $derived(isAdmin(group, me) || (rolesOn(group) && ["manage_server", "manage_roles", "manage_rooms", "kick_members", "ban_members", "mute_members"].some((p) => hasPerm(group, me, p as PermKey))));
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
      const st = get(groupPanelStart);
      if (st) {
        groupPanelStart.set(null);
        if (st.room) { editRoomId = st.room; permRoomId = st.room; }
        stack = st.page === "rooms" ? ["info"] : ["info", "rooms"];
        page = st.page as Page;
      }
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
  const isVid = (u: string) => /\.(webm|mp4|mov|mkv|m4v)(\?|$)/i.test(u);
  async function pickGroupImage(kind: "photo" | "banner") {
    if (!group) return;
    try {
      const result = await openFileDialog({
        multiple: false, directory: false, title: tr(kind === "photo" ? "Pick a photo" : "Pick a banner"),
        filters: kind === "banner"
          ? [{ name: tr("Images, GIFs, short videos"), extensions: ["png", "jpg", "jpeg", "webp", "gif", "mp4", "webm", "mov", "mkv", "m4v"] }]
          : [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }],
      });
      const path = Array.isArray(result) ? (result[0] ?? null) : result;
      if (!path) return;
      let file = path;
      let keep = false;
      const animated = /\.(gif|mp4|webm|mov|mkv|m4v)$/i.test(path);
      if (kind === "banner" && animated) {
        // GIFs and clips become a silent 3:1 WebM loop (≤ 8 s, ≤ 4 MB).
        uploading = kind;
        file = await api.prepareBannerVideo(path);
        keep = true;
      } else if (kind === "photo" && animated && !path.toLowerCase().endsWith(".gif")) {
        showToast(tr("A group photo can be a picture or a GIF"));
        return;
      } else if (!animated) {
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
      const m = await api.uploadAttachment(group.id, file, keep);
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

  // ---- one room's settings (Telegram "Edit topic") ------------------------
  /** Telegram's topic colours: a room without an emoji shows a tinted mark. */
  const ROOM_TINTS = ["#6FB9F0", "#FFD67E", "#CB86DB", "#8EEE98", "#FF93B2", "#FB6F5F"];
  const ROOM_EMOJI: { title: string; list: string[] }[] = [
    { title: "Talk", list: ["💬", "🗨️", "📢", "📣", "📌", "❓", "❗", "💡", "📝", "📰", "📜", "🗳️", "🤝", "👋", "🙏", "💭"] },
    { title: "Fun", list: ["🎮", "🕹️", "🎲", "🎯", "🎨", "🎬", "🎵", "🎧", "🎤", "📸", "🍿", "🎉", "🎁", "🏆", "⚽", "🏀"] },
    { title: "Work", list: ["💼", "📊", "📈", "🗂️", "📅", "⏰", "🛠️", "⚙️", "💻", "🖥️", "📱", "🧪", "🔬", "📚", "🎓", "✅"] },
    { title: "Life", list: ["🍕", "🍔", "☕", "🍵", "🍰", "🍺", "✈️", "🚗", "🏠", "🌍", "🌙", "☀️", "🌸", "🌿", "🐱", "🐶"] },
    { title: "Signs", list: ["❤️", "🔥", "⭐", "⚡", "💎", "🚀", "👑", "🛡️", "🔒", "🔔", "🆕", "🆘", "♻️", "🧩", "🎗️", "🏳️"] },
  ];
  let editRoomId = $state("");
  let editRoom = $derived((group?.rooms ?? []).find((r) => r.id === editRoomId));
  let rName = $state(""), rEmoji = $state(""), rColor = $state(""), rTopic = $state(""), rReadonly = $state(false);
  let rLoaded = "";
  $effect(() => {
    if (page !== "room-edit" || !editRoom) { if (page !== "room-edit") rLoaded = ""; return; }
    if (rLoaded === editRoom.id) return;
    rLoaded = editRoom.id;
    rName = editRoom.name; rEmoji = editRoom.emoji ?? ""; rColor = editRoom.color ?? "";
    rTopic = editRoom.topic ?? ""; rReadonly = !!editRoom.readonly;
  });
  const canRooms = $derived(!!group && (can(group, me, "change_info") || hasPerm(group, me, "manage_rooms")));
  const roomDirty = $derived(!!editRoom && (rName.trim() !== editRoom.name || rEmoji !== (editRoom.emoji ?? "") || rColor !== (editRoom.color ?? "")
    || rTopic.trim() !== (editRoom.topic ?? "") || rReadonly !== !!editRoom.readonly));
  async function saveRoom() {
    if (!editRoom) return;
    const name = rName.trim().slice(0, 32);
    if (!name) { showToast(tr("Name can't be empty")); return; }
    const was = editRoom.name;
    await saveRooms(patchRoom(editRoom.id, {
      name, emoji: rEmoji || undefined, color: rColor || undefined, topic: rTopic.trim().slice(0, 240) || undefined,
      readonly: editRoom.kind === "text" ? rReadonly : undefined,
    }), name !== was ? `renamed room ${was} to ${name}` : `edited room ${name}`);
    rLoaded = "";
    showToast(tr("Saved"));
  }
  async function deleteRoom(r: Room) {
    if (!(await askConfirm({ title: tr("Delete room?"), text: tr("“{room}” disappears for everyone. Its messages move to the first room.", { room: r.name }), ok: tr("Delete"), danger: true }))) return;
    await saveRooms(roomList.filter((x) => x.id !== r.id), `deleted room ${r.name}`);
    if (page === "room-edit") back();
  }

  // ---- roles (Discord) -------------------------------------------------------
  let roleList = $derived(rolesOf(group));
  let editRoleId = $state("");
  let editRole = $derived(roleList.find((r) => r.id === editRoleId));
  let roleName = $state("");
  $effect(() => { if (page === "role-edit" && editRole) roleName = editRole.name; });
  const canRoles = $derived(!!group && (isAdmin(group, me) || hasPerm(group, me, "manage_roles")));
  const myPerms = $derived(new Set(group ? PERM_GROUPS.flatMap((x) => x.perms.map((p) => p.key)).filter((p) => hasPerm(group, me, p)) : []));
  async function enableRoles() {
    await commit((g) => {
      g.roles = [
        { id: "mod", name: tr("Moderator"), color: "#e91e63", perms: ["manage_messages", "kick_members", "mute_members", "view_log"], hoist: true, mentionable: true },
        { id: "member", name: tr("Member"), color: "#3498db", perms: [], hoist: false },
        everyoneRole(),
      ];
    }, "turned on roles");
  }
  async function saveRoles(next: Role[], log?: string) { await commit((g) => { g.roles = next; }, log); }
  function patchRole(id: string, p: Partial<Role>) { return roleList.map((r) => (r.id === id ? { ...r, ...p } : r)); }
  async function addRole() {
    let n = 1, id = "role";
    while (roleList.some((r) => r.id === id)) id = `role${++n}`;
    // New roles go just below my highest role (or at the top for owners).
    const at = Math.max(0, rank(group, me) + 1);
    const next = [...roleList];
    next.splice(Math.min(at, next.length - 1), 0, { id, name: tr("new role"), color: ROLE_COLORS[(roleList.length * 5) % ROLE_COLORS.length], perms: [] });
    await saveRoles(next, "created a role");
    editRoleId = id; go("role-edit");
  }
  function moveRole(i: number, d: number) {
    const a = [...roleList], j = i + d;
    if (j < 0 || j >= a.length - 1 || i >= a.length - 1) return;   // @everyone stays last
    if (!canManageRole(group, me, a[i].id) || !canManageRole(group, me, a[j].id)) return;
    [a[i], a[j]] = [a[j], a[i]];
    void saveRoles(a);
  }
  function togglePermOf(r: Role, p: PermKey) {
    const has = r.perms.includes(p);
    void saveRoles(patchRole(r.id, { perms: has ? r.perms.filter((x) => x !== p) : [...r.perms, p] }));
  }
  async function deleteRole(r: Role) {
    await commit((g) => {
      g.roles = (g.roles ?? []).filter((x) => x.id !== r.id);
      g.members = g.members.map((m) => (m.roles?.includes(r.id) ? { ...m, roles: m.roles.filter((x) => x !== r.id) } : m));
    }, `deleted role ${r.name}`);
    back();
  }
  async function toggleMemberRole(u: string, roleId: string) {
    const cur = memberRoleIds(group, u);
    const has = cur.includes(roleId);
    await setMemberField(u, { roles: has ? cur.filter((x) => x !== roleId) : [...cur, roleId] }, `${has ? "took" : "gave"} a role ${has ? "from" : "to"} @${u}`);
  }
  // Room overrides: tri-state per role.
  let permRoomId = $state("");
  let permRoom = $derived(roleList.length ? (group?.rooms ?? []).find((r) => r.id === permRoomId) : undefined);
  function ovState(room: Room, roleId: string, p: PermKey): "allow" | "deny" | "" {
    const o = room.overrides?.[roleId];
    return o?.allow.includes(p) ? "allow" : o?.deny.includes(p) ? "deny" : "";
  }
  async function cycleOverride(room: Room, roleId: string, p: PermKey) {
    const st = ovState(room, roleId, p);
    const nextSt = st === "" ? "allow" : st === "allow" ? "deny" : "";
    const ov = { ...(room.overrides ?? {}) };
    const o = { allow: (ov[roleId]?.allow ?? []).filter((x) => x !== p), deny: (ov[roleId]?.deny ?? []).filter((x) => x !== p) };
    if (nextSt === "allow") o.allow.push(p);
    if (nextSt === "deny") o.deny.push(p);
    if (!o.allow.length && !o.deny.length) delete ov[roleId]; else ov[roleId] = o;
    await commit((g) => { g.rooms = (g.rooms ?? []).map((r) => (r.id === room.id ? { ...r, overrides: ov } : r)); });
  }
  const PERM_LABEL = Object.fromEntries(PERM_GROUPS.flatMap((g) => g.perms.map((p) => [p.key, p.label]))) as Record<PermKey, string>;

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
    requests: "Join requests", clan: "Server tag", log: "Recent actions", rooms: "Rooms", "room-edit": "Room settings",
    roles: "Roles", "role-edit": "Edit role", "room-perms": "Room permissions",
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
          <div class="gbanner" style:background={media.banner && !isVid(media.banner) ? `center / cover no-repeat url("${media.banner}")` : `linear-gradient(135deg, ${group.accent ?? "#223"}, color-mix(in srgb, ${group.accent ?? "#223"} 40%, #000))`}>
            {#if media.banner && isVid(media.banner)}<video class="gb-vid" use:streamSrc={media.banner} muted loop autoplay playsinline></video>{/if}
          </div>
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
                <span class="m-name" style:color={roleColor(group, m.username) || null}>{m.username}{#if m.username === me}<em> {$t("(you)")}</em>{/if}
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
            <div class="me-banner" style:background={media.banner && !isVid(media.banner) ? `center / cover no-repeat url("${media.banner}")` : (group.accent ? group.accent : "var(--surface-2, #2a2f3a)")}>
              {#if media.banner && isVid(media.banner)}<video class="gb-vid" use:streamSrc={media.banner} muted loop autoplay playsinline></video>{/if}
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
            <div class="note">{$t("Photo: PNG, JPG, WebP or a GIF. Banner: a picture, a GIF or a short clip (3–5 s; it becomes a silent loop up to 8 s). Encrypted for members; the relay keeps it while it's being viewed.")}</div>
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
              <ColorInput value={dAccent || "#3fb6a8"} oninput={(v) => (dAccent = v)} title={$t("Custom colour")} disabled={!can(group, me, "change_info")} size={26} />
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
            {@render row("Roles", rolesOn(group) ? `${roleList.length - 1}` : "Off", () => go("roles"), "#EB459E")}
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
              <span class="m-name" style:color={roleColor(group, m.username) || null}>{m.username}{#if m.username === me}<em> {$t("(you)")}</em>{/if}
                {#each memberRoles(group, m.username).slice(0, 3) as r (r.id)}<span class="role-pill" style:--rc={r.color || "var(--fg-dim)"}>{r.name}</span>{/each}
                {#if mt?.tag}<span class="mtag" class:admin={mt.admin}>{mt.tag}</span>{/if}
                {#if m.muted}<span class="mtag muted">{$t("restricted")}</span>{/if}</span>
              {#if mt?.title}<span class="m-role" class:owner={m.role === "owner"}>{mt.title}</span>{/if}
            </button>
            {#if openMember === m.username && canRoles && rolesOn(group) && (m.username === me ? isAdmin(group, me) : outranks(group, me, m.username))}
              <div class="macts">
                <div class="ct">{$t("Roles")}</div>
                <div class="role-picks">
                  {#each roleList.filter((r) => r.id !== "everyone") as r (r.id)}
                    {@const on = memberRoleIds(group, m.username).includes(r.id)}
                    <button class="role-pick" class:on style:--rc={r.color || "var(--accent)"} disabled={!canManageRole(group, me, r.id) || busy} onclick={() => toggleMemberRole(m.username, r.id)}>{on ? "✓ " : ""}{r.name}</button>
                  {/each}
                </div>
              </div>
            {/if}
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
            <ColorInput bind:value={clanColor} title={$t("Custom colour")} size={26} />
          </div>
          <div class="abtns">
            <button class="btn" onclick={() => saveClan()}>{$t("Save tag")}</button>
            {#if group.clan}<button class="btn ghost" onclick={() => saveClan(true)}>{$t("Remove")}</button>{/if}
          </div>
          <div class="note">{$t("Like Discord server tags: members can wear it next to their name everywhere, and anyone can click it to ask to join.")}</div>
        </div>

      {:else if page === "roles"}
        {#if !rolesOn(group)}
          <div class="card">
            <div class="note">{$t("Roles work like on a Discord server: named, coloured ranks with their own permissions, a hierarchy, and per-room overrides. Admins and the owner keep full rights.")}</div>
            <button class="btn wide" onclick={enableRoles} disabled={busy || !isAdmin(group, me)}>{$t("Turn on roles")}</button>
          </div>
        {:else}
          <div class="card">
            <div class="note">{$t("Higher roles come first. Members get the permissions of all their roles; the colour of the highest coloured one. You can manage roles below your own.")}</div>
            {#each roleList as r, i (r.id)}
              {@const count = r.id === "everyone" ? group.members.length : group.members.filter((m) => (m.roles ?? []).includes(r.id)).length}
              <div class="role-row">
                <span class="role-dot" style:background={r.color || "var(--fg-dim)"}></span>
                <button class="role-name" onclick={() => { editRoleId = r.id; go("role-edit"); }} disabled={!canManageRole(group, me, r.id) && r.id !== "everyone"}>
                  <b style:color={r.color || null}>{r.name}</b><span class="rs">{$tn(count, "member")}{r.hoist ? " · " + $t("shown separately") : ""}</span>
                </button>
                {#if r.id !== "everyone"}
                  <button class="rr-btn" onclick={() => moveRole(i, -1)} disabled={i === 0 || !canManageRole(group, me, r.id) || !canManageRole(group, me, roleList[i - 1]?.id ?? "")} aria-label={$t("Up")}>↑</button>
                  <button class="rr-btn" onclick={() => moveRole(i, 1)} disabled={i >= roleList.length - 2 || !canManageRole(group, me, r.id)} aria-label={$t("Down")}>↓</button>
                {/if}
              </div>
            {/each}
            {#if canRoles}<button class="btn wide" onclick={addRole} disabled={busy}>＋ {$t("Create role")}</button>{/if}
          </div>
        {/if}

      {:else if page === "role-edit" && editRole}
        {@const locked = editRole.id === "everyone" ? !isAdmin(group, me) && !hasPerm(group, me, "manage_roles") : !canManageRole(group, me, editRole.id)}
        <div class="card">
          {#if editRole.id !== "everyone"}
            <label class="fld"><span>{$t("Role name")}</span>
              <div class="inline"><input class="inp" maxlength="32" bind:value={roleName} disabled={locked} />
                <button class="btn" onclick={() => roleName.trim() && saveRoles(patchRole(editRole!.id, { name: roleName.trim() }))} disabled={locked || busy}>{$t("Set")}</button></div>
            </label>
            <div class="fld"><span>{$t("Colour")}</span>
              <div class="colors">
                <button class="ccell none" class:on={!editRole.color} onclick={() => saveRoles(patchRole(editRole!.id, { color: "" }))} disabled={locked}>∅</button>
                {#each ROLE_COLORS as c (c)}<button class="ccell" class:on={editRole.color === c} style:background={c} onclick={() => saveRoles(patchRole(editRole!.id, { color: c }))} aria-label={c} disabled={locked}></button>{/each}
                <ColorInput value={editRole.color || "#99aab5"} oninput={(v) => saveRoles(patchRole(editRole!.id, { color: v }))} size={26} disabled={locked} />
              </div>
            </div>
            {@render toggle("Show separately", "Members with this role are listed in their own group", !!editRole.hoist, () => saveRoles(patchRole(editRole!.id, { hoist: !editRole!.hoist })), locked || busy)}
            {@render toggle("Mentionable", "Anyone can @mention this role", !!editRole.mentionable, () => saveRoles(patchRole(editRole!.id, { mentionable: !editRole!.mentionable })), locked || busy)}
          {:else}
            <div class="note">{$t("@everyone — the permissions every member has, before other roles add to them.")}</div>
          {/if}
        </div>
        {#each PERM_GROUPS as pg (pg.title)}
          <div class="card">
            <div class="ct">{$t(pg.title)}</div>
            {#each pg.perms as p (p.key)}
              {@render toggle(p.label, p.desc, editRole.perms.includes(p.key), () => togglePermOf(editRole!, p.key), locked || busy || (!myPerms.has(p.key) && !editRole.perms.includes(p.key)))}
            {/each}
          </div>
        {/each}
        {#if editRole.id !== "everyone"}
          <div class="card">
            <div class="ct">{$t("Members with this role")}</div>
            {#each group.members.filter((m) => (m.roles ?? []).includes(editRole!.id)) as m (m.username)}
              <div class="mrow">{@render memberAva(m.username)}<span class="m-name">{m.username}</span>
                {#if !locked}<button class="btn ghost" onclick={() => toggleMemberRole(m.username, editRole!.id)}>{$t("Remove")}</button>{/if}</div>
            {:else}
              <div class="note">{$t("Nobody yet — give it to people in Members.")}</div>
            {/each}
          </div>
          {#if !locked}<button class="danger-btn" onclick={() => deleteRole(editRole!)}>{$t("Delete role")}</button>{/if}
        {/if}

      {:else if page === "room-edit" && editRoom}
        {@const r = editRoom}
        <div class="card re-card">
          <div class="re-hero">
            <span class="re-icon" style:--tint={rColor || ROOM_TINTS[0]} class:emoji={!!rEmoji}>
              {#if rEmoji}{rEmoji}{:else}{r.kind === "voice" ? "🔊" : (rName.trim().charAt(0).toUpperCase() || "#")}{/if}
            </span>
            <div class="re-hero-txt">
              <input class="inp re-name" bind:value={rName} maxlength="32" placeholder={$t("Room name")} disabled={!canRooms} />
              <span class="re-kind">{r.kind === "voice" ? $t("Voice room") : rReadonly ? $t("Announcement room") : $t("Text room")}</span>
            </div>
          </div>
          {#if canRooms}
            <div class="re-lbl">{$t("Icon")}</div>
            <div class="re-tints">
              {#each ROOM_TINTS as c (c)}
                <button class="re-tint" class:on={!rEmoji && (rColor || ROOM_TINTS[0]) === c} style:--tint={c} onclick={() => { rEmoji = ""; rColor = c; }} aria-label={c}>{rName.trim().charAt(0).toUpperCase() || "#"}</button>
              {/each}
              <input class="inp re-own" value={rEmoji} maxlength="4" placeholder="🙂" title={$t("Any emoji")}
                oninput={(e) => { const v = (e.target as HTMLInputElement).value.trim(); rEmoji = [...v].slice(0, 2).join(""); }} />
            </div>
            <div class="re-grid">
              {#each ROOM_EMOJI as cat (cat.title)}
                <div class="re-cat">{$t(cat.title)}</div>
                <div class="re-emojis">
                  {#each cat.list as e (e)}
                    <button class="re-emo" class:on={rEmoji === e} onclick={() => (rEmoji = rEmoji === e ? "" : e)}>{e}</button>
                  {/each}
                </div>
              {/each}
            </div>
          {/if}
        </div>
        {#if r.kind === "text"}
          <div class="card">
            <div class="re-lbl">{$t("Topic")}</div>
            <textarea class="inp re-topic" bind:value={rTopic} maxlength="240" rows="2" placeholder={$t("What this room is for — shown under its name")} disabled={!canRooms}></textarea>
            {@render toggle("Announcements only", "Only admins write in this room; everyone reads", rReadonly, () => (rReadonly = !rReadonly), !canRooms)}
          </div>
        {/if}
        {#if canRooms}
          <button class="btn wide" onclick={saveRoom} disabled={busy || !roomDirty}>{$t("Save")}</button>
          {#if rolesOn(group)}
            <div class="card">{@render row("Room permissions", "", () => { permRoomId = r.id; go("room-perms"); }, "#5865F2")}</div>
          {/if}
          <button class="danger-btn" onclick={() => deleteRoom(r)} disabled={busy || (r.kind === "text" && roomList.filter((x) => x.kind === "text").length < 2)}>{$t("Delete room")}</button>
        {/if}

      {:else if page === "room-perms" && permRoom}
        {@const cols = ROOM_PERMS.filter((p) => permRoom!.kind === "voice" ? ["view_room", "connect", "speak", "video"].includes(p) : !["connect", "speak", "video"].includes(p))}
        <div class="card">
          <div class="note">{$t("Per-room overrides: ✓ allows, ✕ denies, — follows the role. Denies on @everyone apply first, then the member's roles.")}</div>
          <div class="ov-table" style:--cols={cols.length}>
            <span></span>
            {#each cols as p (p)}<span class="ov-h" title={$t(PERM_LABEL[p])}>{$t(PERM_LABEL[p])}</span>{/each}
            {#each roleList as r (r.id)}
              <span class="ov-role" style:color={r.color || null}>{r.name}</span>
              {#each cols as p (p)}
                {@const st = ovState(permRoom!, r.id, p)}
                <button class="ov-cell {st}" onclick={() => cycleOverride(permRoom!, r.id, p)} disabled={busy || !(isAdmin(group, me) || hasPerm(group, me, "manage_rooms"))}>{st === "allow" ? "✓" : st === "deny" ? "✕" : "—"}</button>
              {/each}
            {/each}
          </div>
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
                  onchange={(e) => { const v = (e.target as HTMLInputElement).value.trim().slice(0, 32); if (v) void saveRooms(patchRoom(r.id, { name: v }), `renamed room to ${v}`); }} />
                {#if r.kind === "text"}
                  <button class="rr-btn" class:on={r.readonly} title={$t("Only admins write here")} onclick={() => saveRooms(patchRoom(r.id, { readonly: !r.readonly }))}>🔒</button>
                {:else}
                  <span class="rr-kind">{$t("voice")}</span>
                {/if}
                <button class="rr-btn" onclick={() => moveRoom(i, -1)} disabled={i === 0} aria-label={$t("Up")}>↑</button>
                <button class="rr-btn" onclick={() => moveRoom(i, 1)} disabled={i === roomList.length - 1} aria-label={$t("Down")}>↓</button>
                <button class="rr-btn" onclick={() => { editRoomId = r.id; go("room-edit"); }} title={$t("Room settings")}>⚙</button>
                <button class="rr-btn danger" onclick={() => deleteRoom(r)} aria-label={$t("Delete")}>×</button>
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
  .gb-vid { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; pointer-events: none; z-index: 0; }
  .me-banner { position: relative; overflow: hidden; }
  .me-banner > :global(button) { position: relative; z-index: 1; }
  .gbanner { position: relative; overflow: hidden; height: 120px; margin: -12px -14px 0; animation: gb-in .45s cubic-bezier(.2,.8,.2,1); }
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
  .role-row { display: flex; align-items: center; gap: 8px; padding: 6px 0; border-bottom: 1px solid var(--border); }
  .role-row:last-of-type { border-bottom: none; }
  .role-dot { width: 12px; height: 12px; border-radius: 50%; flex-shrink: 0; }
  .role-name { flex: 1; display: flex; flex-direction: column; align-items: flex-start; border: none; background: none; color: var(--fg); cursor: pointer; padding: 2px 0; font: inherit; text-align: left; }
  .role-name:disabled { cursor: default; }
  .role-name b { font-size: 13.5px; }
  .role-pill { display: inline-block; margin-left: 5px; padding: 0 6px; border-radius: 6px; font-size: 10.5px; font-weight: 700; vertical-align: 1px;
    color: var(--rc); background: color-mix(in srgb, var(--rc) 16%, transparent); }
  .role-picks { display: flex; flex-wrap: wrap; gap: 6px; padding-bottom: 6px; }
  .role-pick { border: 1px solid color-mix(in srgb, var(--rc) 50%, transparent); background: transparent; color: var(--rc); border-radius: 8px; padding: 4px 10px;
    font: inherit; font-size: 12px; font-weight: 700; cursor: pointer; transition: background .15s; }
  .role-pick.on { background: color-mix(in srgb, var(--rc) 22%, transparent); }
  .role-pick:disabled { opacity: .4; cursor: not-allowed; }
  .ov-table { display: grid; grid-template-columns: minmax(80px, 1.2fr) repeat(var(--cols), minmax(34px, 1fr)); gap: 4px; align-items: center; overflow-x: auto; padding-bottom: 8px; }
  .ov-h { font-size: 9.5px; font-weight: 700; color: var(--fg-dim); text-align: center; line-height: 1.15; word-break: break-word; }
  .ov-role { font-size: 12.5px; font-weight: 700; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ov-cell { height: 28px; border-radius: 7px; border: 1px solid var(--border); background: var(--bg-input); color: var(--fg-dim); cursor: pointer; font-weight: 800; }
  .ov-cell.allow { background: rgba(61,220,132,.16); color: #3ddc84; border-color: rgba(61,220,132,.4); }
  .ov-cell.deny { background: rgba(255,77,109,.14); color: #ff6b6b; border-color: rgba(255,77,109,.4); }
  .re-card { padding: 12px; }
  .re-hero { display: flex; gap: 12px; align-items: center; margin-bottom: 6px; }
  .re-icon { width: 56px; height: 56px; flex-shrink: 0; border-radius: 50%; display: grid; place-items: center; font-size: 24px; font-weight: 800;
    color: #fff; background: radial-gradient(circle at 30% 25%, color-mix(in srgb, var(--tint) 70%, #fff), var(--tint)); box-shadow: 0 6px 18px -6px var(--tint);
    transition: background .2s, transform .2s var(--ease-spring, ease); }
  .re-icon.emoji { background: var(--bg-input); box-shadow: none; font-size: 32px; }
  .re-hero-txt { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 4px; }
  .re-name { font-size: 15px; font-weight: 700; }
  .re-kind { font-size: 11.5px; color: var(--fg-dim); padding-left: 2px; }
  .re-lbl { font-size: 11px; font-weight: 800; letter-spacing: .06em; text-transform: uppercase; color: var(--fg-dim); margin: 10px 0 6px; }
  .re-tints { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .re-tint { width: 34px; height: 34px; border-radius: 50%; border: 2px solid transparent; cursor: pointer; color: #fff; font-weight: 800; font-size: 13px;
    background: var(--tint); transition: transform .15s; }
  .re-tint:hover { transform: scale(1.08); }
  .re-tint.on { border-color: var(--fg); box-shadow: 0 0 0 2px var(--bg-surface) inset; }
  .inp.re-own { flex: 0 0 64px; width: 64px; text-align: center; font-size: 18px; padding: 5px; }
  .re-grid { max-height: 240px; overflow-y: auto; margin-top: 8px; padding-right: 2px; }
  .re-cat { font-size: 11px; color: var(--fg-dim); margin: 8px 0 4px; font-weight: 700; }
  .re-emojis { display: grid; grid-template-columns: repeat(8, 1fr); gap: 4px; }
  .re-emo { aspect-ratio: 1; border-radius: 10px; border: 1px solid transparent; background: transparent; font-size: 20px; cursor: pointer; transition: background .12s, transform .12s; }
  .re-emo:hover { background: var(--bg-input); transform: scale(1.1); }
  .re-emo.on { background: var(--accent-soft); border-color: var(--accent); }
  .re-topic { width: 100%; box-sizing: border-box; resize: vertical; font: inherit; font-size: 13px; margin-bottom: 4px; }
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
