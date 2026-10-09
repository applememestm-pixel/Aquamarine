/**
 * Groups / supergroups / channels store. Mirrors the Rust-side group list;
 * refreshed on launch and whenever the backend emits `aqua://groups_updated`.
 */
import { writable, get, derived } from "svelte/store";
import type { Group } from "./types";
import { api, events } from "./tauri";
import { info } from "./stores";
import { can } from "./groupRules";
import { showToast } from "./components/Toast.svelte";
import { allowed } from "./privacy";

export const groups = writable<Group[]>([]);

/** Map id -> group, for quick lookups in components. */
export const groupMap = derived(groups, ($g) => {
  const m: Record<string, Group> = {};
  for (const g of $g) m[g.id] = g;
  return m;
});

/** Groups we know about (null until the first load, so startup isn't "new"). */
let known: Set<string> | null = null;
/** Groups we asked to join — being added to those is always welcome. */
const requested = new Set<string>();

export async function requestJoinGroup(gid: string, admin: string, title: string, note: string) {
  requested.add(gid);
  await api.groupRequestJoin(gid, admin, title, note);
}

export async function refreshGroups() {
  let list: Group[];
  try {
    list = await api.listGroups();
  } catch (e) {
    console.warn("listGroups failed", e);
    return;
  }
  const me = get(info)?.username ?? "";
  const prev = known;
  known = new Set(list.map((g) => g.id));
  if (prev && me) {
    // Privacy → Invites: leave groups added by people who may not add us.
    const unwanted = list.filter((g) => !prev.has(g.id) && g.owner !== me && !requested.has(g.id) && !allowed("invites", g.owner));
    for (const g of unwanted) {
      void api.leaveGroup(g.id).catch(() => {});
      showToast(`Left “${g.title}” — @${g.owner} can't add you to groups (Privacy → Invites)`, 6000);
    }
    if (unwanted.length) {
      const drop = new Set(unwanted.map((g) => g.id));
      list = list.filter((g) => !drop.has(g.id));
    }
  }
  groups.set(list);
}

export function groupById(id: string): Group | undefined {
  return get(groups).find((g) => g.id === id);
}

export function isGroupId(id: string): boolean {
  return id.startsWith("grp_");
}

/**
 * Someone asked to join a group we run. Public groups without "approve new
 * members" let them straight in (Telegram behaviour); otherwise the request
 * waits on the group's Requests page.
 */
async function onJoinRequest(gid: string, from: string) {
  await refreshGroups();
  const g = groupById(gid);
  const me = get(info)?.username ?? "";
  if (!g || !me || !can(g, me, "invite_users")) return;
  if ((g.banned ?? []).includes(from)) return;
  if (g.members.some((m) => m.username === from)) return;
  if (g.settings.is_public && !g.settings.join_requests) {
    const next: Group = JSON.parse(JSON.stringify(g));
    next.requests = (next.requests ?? []).filter((r) => r.username !== from);
    next.members.push({ username: from, role: "member" });
    next.log = [...(next.log ?? []), { at: new Date().toLocaleString(), by: me, text: `@${from} joined via invite link` }].slice(-200);
    try {
      await api.updateGroup(next);
      await refreshGroups();
    } catch (e) {
      console.warn("auto-approve failed", e);
    }
    return;
  }
  showToast(`@${from} asked to join “${g.title}” — open its settings → Join requests`, 6000);
}

/** Wire the live refresh. Call once at startup. */
export function initGroups() {
  void refreshGroups();
  void events.onGroupsUpdated(() => refreshGroups());
  void events.onGroupJoinRequest((p) => { void onJoinRequest(p.gid, p.from); });
}
