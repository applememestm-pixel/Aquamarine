/**
 * Discord-style roles for supergroups.
 *
 *  - `group.roles` is ordered: index 0 is the highest role. The last entry
 *    is always `@everyone` (id "everyone"), which every member has.
 *  - A member's roles are `member.roles` (ids).
 *  - Permissions are the union of the member's roles; `administrator`
 *    grants everything; the owner (and legacy owner/admin members) has
 *    everything.
 *  - Rooms can override per role: first @everyone's deny/allow, then the
 *    union of the member's other roles' deny, then their allow (Discord).
 *  - You can only manage roles below your highest role, and only grant
 *    permissions you have yourself (the backend checks the same when it
 *    accepts a snapshot — state.rs `role_change_ok`).
 *
 * The Rust side (state.rs) mirrors `permsOf` for what it enforces.
 */
import type { Group, Room } from "./types";

export type PermKey =
  | "administrator" | "manage_server" | "manage_roles" | "manage_rooms" | "view_log"
  | "kick_members" | "ban_members" | "mute_members" | "invite_members"
  | "view_room" | "send_messages" | "attach_files" | "embed_links" | "add_reactions"
  | "send_polls" | "use_stickers" | "mention_everyone" | "manage_messages"
  | "connect" | "speak" | "video";

export interface Role {
  id: string;
  name: string;
  color: string;      // "" = no colour
  perms: PermKey[];
  hoist?: boolean;    // shown separately in the member list
  mentionable?: boolean;
}
export interface Override { allow: PermKey[]; deny: PermKey[] }

export const PERM_GROUPS: { title: string; perms: { key: PermKey; label: string; desc: string }[] }[] = [
  { title: "General", perms: [
    { key: "administrator", label: "Administrator", desc: "Every permission, in every room. Grant with care." },
    { key: "manage_server", label: "Manage server", desc: "Name, photo, banner, rules, settings." },
    { key: "manage_roles", label: "Manage roles", desc: "Create and edit roles below their own; give them to members." },
    { key: "manage_rooms", label: "Manage rooms", desc: "Create, rename, reorder and delete rooms; room permissions." },
    { key: "view_log", label: "View recent actions", desc: "See the admin log." },
  ] },
  { title: "Members", perms: [
    { key: "invite_members", label: "Invite members", desc: "Add people and share the invite link." },
    { key: "kick_members", label: "Kick members", desc: "Remove members below their own role." },
    { key: "ban_members", label: "Ban members", desc: "Ban members below their own role." },
    { key: "mute_members", label: "Mute members", desc: "Restrict members from sending." },
  ] },
  { title: "Text rooms", perms: [
    { key: "view_room", label: "View rooms", desc: "See the room and read its messages." },
    { key: "send_messages", label: "Send messages", desc: "" },
    { key: "attach_files", label: "Attach files", desc: "Photos, videos, files, voice messages." },
    { key: "embed_links", label: "Embed links", desc: "" },
    { key: "use_stickers", label: "Stickers & GIFs", desc: "" },
    { key: "add_reactions", label: "Add reactions", desc: "" },
    { key: "send_polls", label: "Create polls", desc: "" },
    { key: "mention_everyone", label: "Mention @everyone", desc: "" },
    { key: "manage_messages", label: "Manage messages", desc: "Delete others' messages, pin messages." },
  ] },
  { title: "Voice rooms", perms: [
    { key: "connect", label: "Connect", desc: "Join voice rooms." },
    { key: "speak", label: "Speak", desc: "" },
    { key: "video", label: "Video & screen", desc: "Camera and screen sharing." },
  ] },
];
export const ALL_PERMS: PermKey[] = PERM_GROUPS.flatMap((g) => g.perms.map((p) => p.key));
/** What @everyone may do in a fresh server. */
export const EVERYONE_DEFAULT: PermKey[] = ["view_room", "send_messages", "attach_files", "embed_links", "use_stickers", "add_reactions", "send_polls", "invite_members", "connect", "speak", "video"];
/** Room permissions that can be overridden per role. */
export const ROOM_PERMS: PermKey[] = ["view_room", "send_messages", "attach_files", "embed_links", "add_reactions", "use_stickers", "manage_messages", "connect", "speak", "video"];

export function everyoneRole(): Role { return { id: "everyone", name: "@everyone", color: "", perms: [...EVERYONE_DEFAULT] }; }
export const ROLE_COLORS = ["#1abc9c", "#2ecc71", "#3498db", "#9b59b6", "#e91e63", "#f1c40f", "#e67e22", "#e74c3c", "#95a5a6", "#607d8b", "#11806a", "#206694", "#71368a", "#ad1457", "#c27c0e", "#a84300"];

/** Roles on (supergroups that have a role list). */
export function rolesOn(g: Group | undefined): boolean {
  return !!g && g.kind === "supergroup" && Array.isArray(g.roles) && g.roles.length > 0;
}
export function rolesOf(g: Group | undefined): Role[] {
  if (!rolesOn(g)) return [];
  const list = g!.roles as Role[];
  return list.some((r) => r.id === "everyone") ? list : [...list, everyoneRole()];
}
export function memberRoleIds(g: Group | undefined, u: string): string[] {
  const m = g?.members.find((x) => x.username === u);
  return Array.isArray(m?.roles) ? (m!.roles as string[]) : [];
}
function legacyAll(g: Group | undefined, u: string): boolean {
  const r = g?.members.find((x) => x.username === u)?.role;
  return r === "owner" || r === "admin" || g?.owner === u;
}
/** The member's roles, highest first (without @everyone). */
export function memberRoles(g: Group | undefined, u: string): Role[] {
  const ids = new Set(memberRoleIds(g, u));
  return rolesOf(g).filter((r) => r.id !== "everyone" && ids.has(r.id));
}
/** Position of the member's highest role (lower = higher); owner/admin = -1. */
export function rank(g: Group | undefined, u: string): number {
  if (legacyAll(g, u)) return -1;
  const roles = rolesOf(g);
  const ids = new Set(memberRoleIds(g, u));
  const i = roles.findIndex((r) => ids.has(r.id));
  return i < 0 ? roles.length : i;
}
export function roleColor(g: Group | undefined, u: string): string {
  return memberRoles(g, u).find((r) => r.color)?.color ?? "";
}

/** Effective permissions of `u` (in `room`, when given). */
export function permsOf(g: Group | undefined, u: string, room?: Room | null): Set<PermKey> {
  if (!g) return new Set();
  if (legacyAll(g, u)) return new Set(ALL_PERMS);
  const roles = rolesOf(g);
  const everyone = roles.find((r) => r.id === "everyone") ?? everyoneRole();
  const mine = memberRoles(g, u);
  const set = new Set<PermKey>([...everyone.perms, ...mine.flatMap((r) => r.perms)]);
  if (set.has("administrator")) return new Set(ALL_PERMS);
  const ov = (room as (Room & { overrides?: Record<string, Override> }) | null | undefined)?.overrides;
  if (ov) {
    const e = ov.everyone;
    if (e) { e.deny.forEach((p) => set.delete(p)); e.allow.forEach((p) => set.add(p)); }
    const deny = new Set<PermKey>(), allow = new Set<PermKey>();
    for (const r of mine) { const o = ov[r.id]; if (o) { o.deny.forEach((p) => deny.add(p)); o.allow.forEach((p) => allow.add(p)); } }
    deny.forEach((p) => set.delete(p));
    allow.forEach((p) => set.add(p));
  }
  return set;
}
export function hasPerm(g: Group | undefined, u: string, p: PermKey, room?: Room | null): boolean {
  return permsOf(g, u, room).has(p);
}
/** Can `me` edit `role` (strictly below my highest role)? */
export function canManageRole(g: Group | undefined, me: string, roleId: string): boolean {
  if (!hasPerm(g, me, "manage_roles") && !legacyAll(g, me)) return false;
  if (legacyAll(g, me)) return true;
  const i = rolesOf(g).findIndex((r) => r.id === roleId);
  return i > rank(g, me);
}
/** Can `me` act on member `u` (kick, ban, roles): `u` ranks below `me`. */
export function outranks(g: Group | undefined, me: string, u: string): boolean {
  if (me === u) return false;
  if (g?.owner === u) return false;
  if (g?.owner === me) return true;
  return rank(g, me) < rank(g, u);
}
