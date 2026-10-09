/**
 * Group / channel rules shared by the UI (the backend enforces the same on
 * receive — see state.rs `may_post` / `accept_group_snapshot`).
 */
import type { AdminRights, Group, GroupMember, Perm } from "./types";

export function memberOf(g: Group | undefined, u: string): GroupMember | undefined {
  return g?.members.find((m) => m.username === u);
}
export function isOwner(g: Group | undefined, u: string): boolean {
  return !!g && memberOf(g, u)?.role === "owner";
}
export function isAdmin(g: Group | undefined, u: string): boolean {
  const r = memberOf(g, u)?.role;
  return r === "owner" || r === "admin";
}

export const ALL_RIGHTS: (keyof AdminRights)[] = [
  "change_info", "post_messages", "edit_messages", "delete_messages", "ban_users",
  "invite_users", "pin_messages", "manage_tags", "add_admins", "anonymous",
];

/** Admin right check; owners have all, admins default to all but add_admins/anonymous. */
export function can(g: Group | undefined, u: string, right: keyof AdminRights): boolean {
  const m = memberOf(g, u);
  if (!m) return false;
  if (m.role === "owner") return true;
  if (m.role !== "admin") return false;
  const r = m.rights;
  if (!r) return right !== "add_admins" && right !== "anonymous";
  return !!r[right];
}

export function perm(g: Group, key: keyof Group["settings"]): Perm {
  const v = (g.settings as unknown as Record<string, unknown>)[key];
  return v === "admins" ? "admins" : "all";
}

/** Why the current user can't post here (null = can post). */
export function postBlock(g: Group | undefined, me: string): string | null {
  if (!g) return null;
  const m = memberOf(g, me);
  if (!m) return "You are not a member of this chat.";
  if ((g.banned ?? []).includes(me)) return "You were removed from this chat.";
  if (isAdmin(g, me)) {
    if (g.kind === "channel" && !can(g, me, "post_messages")) return "Your admin rights don't include posting.";
    return null;
  }
  if (g.kind === "channel") return "channel";
  if (m.muted) return "An admin restricted you from sending messages.";
  if (g.settings.send_messages === "admins") return "Only admins can send messages in this group.";
  return null;
}

export function canSendKind(g: Group | undefined, me: string, kind: "poll" | "sticker" | "media" | "voice" | "link"): boolean {
  if (!g || isAdmin(g, me)) return true;
  const key = kind === "poll" ? "send_polls" : kind === "sticker" ? "send_stickers" : kind === "voice" ? "send_voice" : kind === "link" ? "embed_links" : "send_media";
  return perm(g, key as keyof Group["settings"]) !== "admins";
}

/** Member tag + whether it's an admin's (shown in a different colour). */
export function tagOf(g: Group | undefined, u: string): { tag: string; admin: boolean; title: string } | null {
  const m = memberOf(g, u);
  if (!m) return null;
  const admin = m.role === "owner" || m.role === "admin";
  const title = m.title || (m.role === "owner" ? "owner" : m.role === "admin" ? "admin" : "");
  if (!m.tag && !title) return null;
  return { tag: m.tag ?? "", admin, title };
}

export function shareLink(g: Group): string {
  const q = new URLSearchParams({ admin: g.owner, title: g.title });
  return `aqua://join/${g.id}?${q.toString()}`;
}

export function parseJoinLink(s: string): { gid: string; admin: string; title: string } | null {
  const m = /^aqua:\/\/join\/(grp_[A-Za-z0-9_-]+)\?(.*)$/.exec(s.trim());
  if (!m) return null;
  const q = new URLSearchParams(m[2]);
  const admin = q.get("admin") ?? "";
  if (!admin) return null;
  return { gid: m[1], admin, title: q.get("title") ?? "group" };
}
