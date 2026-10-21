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
import { extraAvatars } from "./people";
import { mediaPath } from "./media";
import { assetUrl } from "./tauri";
import type { ArticleMedia } from "./types";

export const groups = writable<Group[]>([]);

/** Decrypted local URLs of group photos / banners: gid → { photo, banner }. */
export const groupMedia = writable<Record<string, { photo?: string; banner?: string }>>({});
/** cid already resolved per gid+slot, so list refreshes don't re-decrypt. */
const resolvedCid = new Map<string, string>();
groups.subscribe((list) => {
  for (const g of list) {
    for (const k of ["photo", "banner"] as const) {
      const slot = `${g.id}|${k}`;
      const m = g[k] as ArticleMedia | null | undefined;
      if (!m?.cid) {
        if (!resolvedCid.has(slot)) continue;
        resolvedCid.delete(slot);
        groupMedia.update((x) => ({ ...x, [g.id]: { ...x[g.id], [k]: undefined } }));
        if (k === "photo") extraAvatars.update((x) => { const { [g.id]: _, ...rest } = x; return rest; });
        continue;
      }
      if (resolvedCid.get(slot) === m.cid) continue;
      resolvedCid.set(slot, m.cid);
      void mediaPath(m).then((path) => {
        const url = assetUrl(path);
        groupMedia.update((x) => ({ ...x, [g.id]: { ...x[g.id], [k]: url } }));
        if (k === "photo") extraAvatars.update((x) => ({ ...x, [g.id]: url }));
      }).catch(() => resolvedCid.delete(slot));
    }
  }
});

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
  archiveEnded(list);
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
/** Event rooms that ended and should go to the archive (once each). */
function archiveEnded(list: Group[]) {
  let done: string[] = [];
  try { done = JSON.parse(localStorage.getItem("aqua.endedArchived") ?? "[]"); } catch { /* ignore */ }
  const now = Date.now();
  let changed = false;
  for (const g of list) {
    const end = g.settings.ends_at;
    if (!end || now < end || (g.settings.end_action ?? "archive") !== "archive" || done.includes(g.id)) continue;
    void import("./chatPrefs").then((p) => p.setArchived(g.id, true));
    done.push(g.id);
    changed = true;
  }
  if (changed) try { localStorage.setItem("aqua.endedArchived", JSON.stringify(done.slice(-500))); } catch { /* ignore */ }
}

export function initGroups() {
  void refreshGroups();
  setInterval(() => archiveEnded(Object.values(get(groupMap))), 60_000);
  void events.onGroupsUpdated(() => refreshGroups());
  void events.onGroupJoinRequest((p) => { void onJoinRequest(p.gid, p.from); });
}
